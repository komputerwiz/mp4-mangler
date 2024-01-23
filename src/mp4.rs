use std::io::{self, Read, Seek, ErrorKind, SeekFrom};
use mp4::{BoxHeader, BoxType};

/// A SAX-style visitor/parser for reading MP4 boxes
pub trait Mp4Visitor {
	fn start_box(&mut self, header: &BoxHeader, reader: &mut impl Read) -> io::Result<()>;
	fn end_box(&mut self, _typ: &BoxType) {}
}

pub fn read_box<R: Read + Seek>(mut reader: R, end: u64, visitor: &mut impl Mp4Visitor) -> io::Result<R> {
	let mut current = reader.stream_position()?;
	while current < end {
		log::debug!("reading box header");
		let header = read_header(&mut reader)?;

		let mut box_end = current + header.size;
		if box_end > end {
			log::error!("declared box size overflows container by {} B", box_end - end);
			box_end = end;
		}

		// save current position
		let payload_start = reader.stream_position()?;
		// limit visitor's reader to just the contents of this box
		let mut sub_reader = reader.take(box_end - current);
		visitor.start_box(&header, &mut sub_reader)?;
		reader = sub_reader.into_inner();
		// restore current position
		reader.seek(SeekFrom::Start(payload_start))?;

		match header.name {
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
				=> {
					log::trace!("skipping box");
					reader.seek(SeekFrom::Start(box_end))?;
				},
			_ => {
				reader = read_box(reader, box_end, visitor)?;
			}
		}

		visitor.end_box(&header.name);

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

			// Subtract the length of the serialized largesize, as callers assume `size - HEADER_SIZE` is the length
			// of the box data. Disallow `largesize < 16`, or else a largesize of 8 will result in a BoxHeader::size
			// of 0, incorrectly indicating that the box data extends to the end of the stream.
			size: match largesize {
				0 => 0,
				1..=15 => return Err(std::io::Error::new(ErrorKind::InvalidData, "64-bit box size too small")),
				16..=u64::MAX => largesize - 8,
			},
		})
	} else {
		Ok(BoxHeader {
			name: BoxType::from(typ),
			size: size as u64,
		})
	}
}
