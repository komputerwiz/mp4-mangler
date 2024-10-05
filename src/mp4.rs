use std::io::{self, Read, Seek, ErrorKind, SeekFrom};
use mp4::{BoxHeader, BoxType};

/// A SAX-style visitor/parser for reading MP4 boxes
pub trait Mp4Visitor {
	fn start_box(&mut self, _header: &BoxHeader, _corrected_size: Option<u64>) -> io::Result<()> { Ok(()) }
	fn data(&mut self, _reader: &mut impl Read) -> io::Result<()> { Ok(()) }
	fn end_box(&mut self, _typ: &BoxType) -> io::Result<()> { Ok(()) }
}

pub fn read_box<R: Read + Seek>(mut reader: R, end: u64, visitor: &mut impl Mp4Visitor) -> io::Result<R> {
	// A box is simply a header followed by content.
	// The header includes the size (in bytes) and type of the box, and has 2 different forms depending on the size:
	//
	// if size <= u32::MAX  (8-byte header),
	//
	//   box {
	//     header {
	//       size: u32,      // big endian
	//       type: [u8; 4],  // four ASCII chars
	//     },
	//     content: [u8; header.size],
	//   }
	//
	// if size > u32::MAX  (16-byte header),
	//
	//   header {
	//     size: u32,       // big endian, set to 1
	//     type: [u8; 4],   // four ASCII chars
	//     largesize: u64,  // big endian
	//   },
	//   content: [u8; header.largesize]
	//
	// The box size declared in the header includes the size of the header itself,
	// so the header of an empty box (i.e., with no content) will have a declared size of 8 bytes

	// The stream is currently positioned at the start of the header, and `current` captures this position.
	// `end` captures the end of the current context, which could be the file itself or the parent box.
	//
	//   ┌─ current                     end ─┐
	//   │                                   │
	//   │ header │ content │ other stuff... │
	//   ^
	let mut current = reader.stream_position()?;

	// Boxes are composite, meaning the contents of a box can be additional (sub-)boxes.
	// Hence, read them iteratively and recursively to catch all of them.
	while current < end {
		log::debug!("reading box header");
		let header = read_header(&mut reader)?;

		// The stream is now positioned at the start of the content.
		// In a corrupted file, the size declared in the header could potentially overflow the end.
		// The following situation is expected and desirable...
		//
		//   ┌─ current         ┌─ box_end   end ─┐
		//   │                  │                 │
		//   │ header │ content │ other stuff...  │
		//            ^
		// But we want to catch the following case to correct for it:
		//
		//   ┌─ current   end? ─┐  box_end? ─┐
		//   │                  │            │
		//   │ header │ content │      oops! │
		//            ^
		let mut box_end = current + header.size;
		let corrected_size = if box_end > end {
			log::error!("declared {} box size overflows container by {} B", header.name, box_end - end);
			box_end = end;
			Some(box_end - current)
		} else {
			None
		};

		visitor.start_box(&header, corrected_size)?;

		match header.name {
			BoxType::FreeBox => {
				log::trace!("skipping over 'free' box");
				reader.seek(SeekFrom::Start(box_end))?;
			},
			BoxType::ElstBox
				| BoxType::FtypBox
				| BoxType::HdlrBox
				| BoxType::MdatBox
				| BoxType::MdhdBox
				| BoxType::MvhdBox
				| BoxType::TkhdBox
				| BoxType::VmhdBox
				| BoxType::DrefBox
				| BoxType::StsdBox
				| BoxType::SttsBox
				| BoxType::StssBox
				| BoxType::CttsBox
				| BoxType::StscBox
				| BoxType::StszBox
				| BoxType::StcoBox
				| BoxType::MetaBox
				| BoxType::SmhdBox
				| BoxType::Co64Box
				| BoxType::UdtaBox
				| BoxType::UnknownBox(0x636c6566) // clef
				| BoxType::UnknownBox(0x70726f66) // prof
				| BoxType::UnknownBox(0x656e6f66) // enof
				| BoxType::UnknownBox(0x63736c67) // cslg
				| BoxType::UnknownBox(0x73647470) // sdtp
				| BoxType::UnknownBox(0x73677064) // sgpd
				| BoxType::UnknownBox(0x73626770) // sbgp
				| BoxType::UnknownBox(0x676d696e) // gmin
				| BoxType::UnknownBox(0x6e6d6864) // nmhd
				=> {
					// limit visitor's reader to just the contents of this box
					let content_start = reader.stream_position()?;
					let mut sub_reader = reader.take(box_end - content_start);
					visitor.data(&mut sub_reader)?;
					reader = sub_reader.into_inner();

					// skip to the end of this box
					log::trace!("not recursing into 'data-only' {} box", header.name);
					reader.seek(SeekFrom::Start(box_end))?;
				},
			_ => {
				// traverse all other boxes recursively
				log::trace!("descending recursively into {} box", header.name);
				reader = read_box(reader, box_end, visitor)?;
			}
		}

		visitor.end_box(&header.name)?;

		current = reader.stream_position()?;
	}

	Ok(reader)
}

fn read_header<R: Read>(reader: &mut R) -> io::Result<BoxHeader> {
	let mut buf = [0u8; 8];
	match reader.read(&mut buf) {
		Ok(sz) => {
			log::trace!("read {} bytes for header: {:0x?}", sz, buf);
			if sz == 0 {
				log::error!("reached EOF");
				return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "reached EOF"));
			}
		},
		Err(e) => {
			log::error!("unable to read header: {}", e);
			Err(e)?;
		},
	};

	// Get size.
	let s = buf[0..4].try_into().unwrap();
	let size = u32::from_be_bytes(s);

	// Get box type string.
	let t = buf[4..8].try_into().unwrap();
	let typ = u32::from_be_bytes(t);

	// Get largesize if size is 1
	if size == 1 {
		reader.read_exact(&mut buf)?;
		let largesize = u64::from_be_bytes(buf);

		Ok(BoxHeader {
			name: BoxType::from(typ),

			// Disallow `largesize < 16`, or else a largesize of 8 will result in a BoxHeader::size of 0,
			// incorrectly indicating that the box data extends to the end of the stream.
			// mp4 crate assumes caller expects content length to be size - 8, but we make no such assumptions here...
			size: match largesize {
				0 => 0,
				1..=15 => return Err(std::io::Error::new(ErrorKind::InvalidData, "64-bit box size too small")),
				16..=u64::MAX => largesize,
			},
		})
	} else {
		Ok(BoxHeader {
			name: BoxType::from(typ),
			size: size as u64,
		})
	}
}

pub trait BoxHeaderExt {
	fn set_size(&mut self, content_size: u64);
}

impl BoxHeaderExt for BoxHeader {
	fn set_size(&mut self, content_size: u64) {
		self.size = content_size + 8;

		if self.size > u32::MAX as u64 {
			// writing will need to use longsize variant
			self.size += 8;
		}
	}
}
