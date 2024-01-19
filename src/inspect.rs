use std::fs::File;
use std::io::{BufReader, Read, Seek, ErrorKind, SeekFrom};
use std::path::Path;

use mp4::{BoxHeader, BoxType, FtypBox, ReadBox, MoofBox, MoovBox, box_start, Mp4Box, HEADER_SIZE};

pub fn inspect(file: &Path) -> Result<(), Box<dyn std::error::Error>> {
	let f = File::open(file)?;
	let file_size = f.metadata()?.len();
	let mut reader = BufReader::new(f);

	read_box(&mut reader, file_size, 0)

	/*
	let mut current = reader.stream_position()?;
	while current < file_size {
		let header = BoxHeader::read(&mut reader)?;

		let box_size = if header.size > file_size {
			log::error!("declared box size overflows file size");
			file_size
		} else {
			header.size
		};

		if box_size == 0 {
			log::warn!("cannot advance past empty block");
			break;
		}

		log::info!("reading box: {}", header.name);

		match header.name {
			BoxType::FtypBox => {
				FtypBox::read_box(&mut reader, box_size)?;
			},
			BoxType::MoovBox => {
				MoovBox::read_box(&mut reader, box_size)?;
			},
			BoxType::MoofBox => {
				MoofBox::read_box(&mut reader, box_size)?;
			},
			n => {
				log::warn!("unhandled box type: {}", n);
				skip_box(&mut reader, box_size)?;
			}
		}
		current = reader.stream_position()?;
	}

	Ok(())
	*/
}

pub fn read_box<R: Read + Seek>(reader: &mut R, size: u64, depth: usize) -> Result<(), Box<dyn std::error::Error>> {
	let mut current = reader.stream_position()?;
	let end = current + size - HEADER_SIZE;
	while current < end {
		log::debug!("reading box header");
		let header = read_header(reader)?;
		// log::info!("found box: {}", header.name);
		println!("{:indent$}{}", "", header.name, indent=depth * 2);

		let box_size = if current > 0 && current - HEADER_SIZE + header.size > end {
			log::error!("declared box size overflows container size");
			size
		} else {
			header.size
		};


		match header.name {
			BoxType::ElstBox => skip_box(reader, box_size)?,
			BoxType::FtypBox => skip_box(reader, box_size)?,
			BoxType::HdlrBox => skip_box(reader, box_size)?,
			BoxType::MdatBox => skip_box(reader, box_size)?,
			BoxType::MdhdBox => skip_box(reader, box_size)?,
			BoxType::MvhdBox => skip_box(reader, box_size)?,
			BoxType::TkhdBox => skip_box(reader, box_size)?,
			BoxType::VmhdBox => skip_box(reader, box_size)?,
			BoxType::DrefBox => skip_box(reader, box_size)?,
			BoxType::StsdBox => skip_box(reader, box_size)?,
			BoxType::SttsBox => skip_box(reader, box_size)?,
			BoxType::StssBox => skip_box(reader, box_size)?,
			BoxType::CttsBox => skip_box(reader, box_size)?,
			BoxType::StscBox => skip_box(reader, box_size)?,
			BoxType::StszBox => skip_box(reader, box_size)?,
			BoxType::StcoBox => skip_box(reader, box_size)?,
			BoxType::MetaBox => skip_box(reader, box_size)?,
			_ => read_box(reader, box_size, depth + 1)?,
		}

		current = reader.stream_position()?;
	}

	Ok(())
}

pub fn read_header<R: Read>(reader: &mut R) -> Result<BoxHeader, Box<dyn std::error::Error>> {
	let mut buf = [0u8; 8];
	match reader.read(&mut buf) {
		Ok(sz) => {
			log::trace!("read {} bytes for header: {:0x?}", sz, buf);
			if sz == 0 {
				log::error!("reached EOF");
				return Err(Box::new(std::io::Error::new(ErrorKind::UnexpectedEof, "reached EOF")));
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
				1..=15 => return Err(Box::new(mp4::Error::InvalidData("64-bit box size too small"))),
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

pub fn skip_box<R: Read + Seek>(reader: &mut R, size: u64) -> Result<(), Box<dyn std::error::Error>> {
	log::trace!("skipping box");
	let start = reader.stream_position()? - HEADER_SIZE;
	match reader.seek(SeekFrom::Start(start + size)) {
		Ok(_) => Ok(()),
		Err(e) => match e.kind() {
			ErrorKind::UnexpectedEof => {
				let current = reader.stream_position()?;
				log::error!("stream ends prematurely: read {} bytes out of {} expected", current - start, size);
				Ok(())
			},
			_ => Err(e)?
		},
	}
}
