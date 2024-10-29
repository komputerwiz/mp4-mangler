use std::io::{self, Read, Seek, ErrorKind, SeekFrom};
use std::fmt;

macro_rules! boxtype {
	($( $name:ident => $value:expr ),*) => {
		#[derive(Clone, Copy, PartialEq, Eq)]
		pub enum BoxType {
			$( $name, )*
			UnknownBox(u32),
		}

		impl BoxType {
			pub fn validate(self) -> bool {
				if let BoxType::UnknownBox(t) = self {
					for byte in t.to_ne_bytes() {
						if byte < 0x61 /* 'a' */ || byte > 0x7A /* 'z' */ {
							return false
						}
					}
				}

				true
			}
		}

		impl From<u32> for BoxType {
			fn from(t: u32) -> BoxType {
				match t {
					$( $value => BoxType::$name, )*
					_ => BoxType::UnknownBox(t),
				}
			}
		}

		impl From<BoxType> for u32 {
			fn from(b: BoxType) -> u32 {
				match b {
					$( BoxType::$name => $value, )*
					BoxType::UnknownBox(t) => t,
				}
			}
		}
	}
}

boxtype! {
	FtypBox => 0x66747970,
	MvhdBox => 0x6d766864,
	MfhdBox => 0x6d666864,
	FreeBox => 0x66726565,
	MdatBox => 0x6d646174,
	MoovBox => 0x6d6f6f76,
	MvexBox => 0x6d766578,
	MehdBox => 0x6d656864,
	TrexBox => 0x74726578,
	EmsgBox => 0x656d7367,
	MoofBox => 0x6d6f6f66,
	TkhdBox => 0x746b6864,
	TfhdBox => 0x74666864,
	TfdtBox => 0x74666474,
	EdtsBox => 0x65647473,
	MdiaBox => 0x6d646961,
	ElstBox => 0x656c7374,
	MdhdBox => 0x6d646864,
	HdlrBox => 0x68646c72,
	MinfBox => 0x6d696e66,
	VmhdBox => 0x766d6864,
	StblBox => 0x7374626c,
	StsdBox => 0x73747364,
	SttsBox => 0x73747473,
	CttsBox => 0x63747473,
	StssBox => 0x73747373,
	StscBox => 0x73747363,
	StszBox => 0x7374737A,
	StcoBox => 0x7374636F,
	Co64Box => 0x636F3634,
	TrakBox => 0x7472616b,
	TrafBox => 0x74726166,
	TrunBox => 0x7472756E,
	UdtaBox => 0x75647461,
	MetaBox => 0x6d657461,
	DinfBox => 0x64696e66,
	DrefBox => 0x64726566,
	UrlBox  => 0x75726C20,
	SmhdBox => 0x736d6864,
	Avc1Box => 0x61766331,
	AvcCBox => 0x61766343,
	Hev1Box => 0x68657631,
	HvcCBox => 0x68766343,
	Mp4aBox => 0x6d703461,
	EsdsBox => 0x65736473,
	Tx3gBox => 0x74783367,
	VpccBox => 0x76706343,
	Vp09Box => 0x76703039,
	DataBox => 0x64617461,
	IlstBox => 0x696c7374,
	NameBox => 0xa96e616d,
	DayBox => 0xa9646179,
	CovrBox => 0x636f7672,
	DescBox => 0x64657363,
	WideBox => 0x77696465,

	// additional types...
	ClefBox => 0x636c6566,
	ProfBox => 0x70726f66,
	EnofBox => 0x656e6f66,
	CslgBox => 0x63736c67,
	SdtpBox => 0x73647470,
	SgpdBox => 0x73677064,
	SbgpBox => 0x73626770,
	GminBox => 0x676d696e,
	NmhdBox => 0x6e6d6864,
	GnreBox => 0x676e7265
}

impl fmt::Debug for BoxType {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}", String::from_utf8_lossy(&Into::<u32>::into(*self).to_be_bytes()))
	}
}

impl fmt::Display for BoxType {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		write!(f, "{}", String::from_utf8_lossy(&Into::<u32>::into(*self).to_be_bytes()))
	}
}

pub struct BoxHeader {
	pub name: BoxType,
	pub size: u64,
	pub longsize: bool,
}

impl BoxHeader {
	pub fn set_size(&mut self, content_size: u64) {
		self.size = content_size + 8;

		if self.size > u32::MAX as u64 {
			// writing will need to use longsize variant
			self.size += 8;
		}
	}
}

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

		// validate header: we expect the header to be 4 ASCII chars
		if !header.name.validate() {
			log::warn!("unable to find valid box header at offset {:#x}; skipping remaining contents", current);
			reader.seek(SeekFrom::Start(end))?;
			return Ok(reader);
		}

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
			// non-recursive boxes
			BoxType::FreeBox
				| BoxType::ElstBox
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
				| BoxType::ClefBox
				| BoxType::ProfBox
				| BoxType::EnofBox
				| BoxType::CslgBox
				| BoxType::SdtpBox
				| BoxType::SgpdBox
				| BoxType::SbgpBox
				| BoxType::GminBox
				| BoxType::NmhdBox
				| BoxType::GnreBox
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

			// recursive boxes
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

			longsize: true,
		})
	} else {
		Ok(BoxHeader {
			name: BoxType::from(typ),
			size: size as u64,
			longsize: false,
		})
	}
}
