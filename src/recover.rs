use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use mp4::{BoxHeader, BoxType};

use crate::mp4::{read_box, Mp4Visitor};

pub fn recover(input: &Path, output: &Path) -> io::Result<()> {
	let in_file = File::open(input)?;
	let in_file_size = in_file.metadata()?.len();
	let reader = io::BufReader::new(in_file);

	let out_file = File::create(output)?;
	let mut writer = io::BufWriter::new(out_file);

	let mut visitor = RecoverVisitor::new(&mut writer);
	read_box(reader, in_file_size, &mut visitor)?;

	Ok(())
}

struct Mp4Box {
	header: BoxHeader,
	data: BoxData,
}

impl Mp4Box {
	pub fn new(header: BoxHeader) -> Self {
		Self {
			header,
			data: BoxData::Empty,
		}
	}

	pub fn write_to(&self, writer: &mut impl io::Write) -> io::Result<u64> {
		let mut size = self.header.write(writer).map_err(|e| match e {
			mp4::Error::IoError(io_error) => io_error,
			e => io::Error::new(io::ErrorKind::Other, e),
		})?;

		size += self.data.write_to(writer)?;

		Ok(size)
	}
}

enum BoxData {
	Empty,
	Raw(Vec<u8>),
	Children(Vec<Mp4Box>),
}

impl BoxData {
	pub fn write_to(&self, writer: &mut impl io::Write) -> io::Result<u64> {
		match self {
			Self::Empty => Ok(0),
			Self::Raw(bytes) => {
				writer.write_all(&bytes)?;
				Ok(bytes.len() as u64)
			},
			Self::Children(children) => {
				let mut sum = 0;
				for child in children {
					sum += child.write_to(writer)?;
				}
				Ok(sum)
			}
		}
	}
}

struct RecoverVisitor<'a> {
	writer: &'a mut dyn io::Write,
	stack: Vec<Mp4Box>,
	found_stco: bool,
	mdat_offset: Option<usize>,
}

impl<'a> RecoverVisitor<'a> {
	fn new(writer: &'a mut (impl io::Write + io::Seek)) -> Self {
		Self {
			writer,
			stack: Vec::new(),
			found_stco: false,
			mdat_offset: None,
		}
	}

}

impl<'a> Mp4Visitor for RecoverVisitor<'a> {
	fn start_box(&mut self, header: &BoxHeader, corrected_size: Option<u64>) -> io::Result<()> {
		let mut dup_header = header.clone();
		if let Some(size) = corrected_size {
			log::warn!("Correcting size mismatch in {} box: header says {} B, but should actually be {} B", dup_header.name, dup_header.size, size);
			dup_header.size = size;
		}

		self.stack.push(Mp4Box::new(dup_header));

		Ok(())
	}

	fn data(&mut self, reader: &mut impl io::Read) -> io::Result<()> {
		let current_box = self.stack.last_mut().unwrap();

		let mut data: Vec<u8> = Vec::new();
		match current_box.header.name {
			BoxType::CttsBox => {
				let mut version_flags = [0u8; 4];
				// version (1 B) and flags (3 B)
				reader.read_exact(&mut version_flags)?;
				data.write(&version_flags)?;

				// entry count (u32)
				let mut entry_count_bytes = [0u8; 4];
				reader.read_exact(&mut entry_count_bytes)?;
				let entry_count = u32::from_be_bytes(entry_count_bytes);

				// read entries ({sample_count: u32, composition_offset: u32})
				let mut entries = Vec::new();
				let mut entry_bytes = [0u8; 8];
				while let Ok(_) = reader.read_exact(&mut entry_bytes) {
					entries.push(entry_bytes);
				}

				if entry_count as usize != entries.len() {
					log::warn!("correcting ctts table length: metadata says {} entries, but should actually be {} entries", entry_count, entries.len());
				}

				data.write(&(entries.len() as u32).to_be_bytes())?;
				for entry in entries {
					data.write(&entry)?;
				}
			},

			BoxType::StszBox => {
				let mut version_flags = [0u8; 4];
				// version (1 B) and flags (3 B)
				reader.read_exact(&mut version_flags)?;

				// sample size (u32)
				let mut sample_size_bytes = [0u8; 4];
				reader.read_exact(&mut sample_size_bytes)?;
				let sample_size = u32::from_be_bytes(sample_size_bytes);

				// entry count (u32)
				let mut entry_count_bytes = [0u8; 4];
				reader.read_exact(&mut entry_count_bytes)?;
				let entry_count = u32::from_be_bytes(entry_count_bytes);

				let mut entries = Vec::new();
				if sample_size == 0 {
					// samples have variable sizes that are stored in the table
					// read entries ({sample_size: u32})
					let mut entry_bytes = [0u8; 4];
					while let Ok(_) = reader.read_exact(&mut entry_bytes) {
						entries.push(entry_bytes);
					}
				}

				if entry_count as usize != entries.len() {
					log::warn!("correcting stsz table length: metadata says {} entries, but should actually be {} entries", entry_count, entries.len());
				}

				data.write(&version_flags)?;
				data.write(&sample_size_bytes)?;
				data.write(&(entries.len() as u32).to_be_bytes())?;
				for entry in entries {
					data.write(&entry)?;
				}
			},

			BoxType::StcoBox => {
				let mut version_flags = [0u8; 4];
				// version (1 B) and flags (3 B)
				reader.read_exact(&mut version_flags)?;
				data.write(&version_flags)?;

				// entry count (u32)
				let mut entry_count_bytes = [0u8; 4];
				reader.read_exact(&mut entry_count_bytes)?;
				let entry_count = u32::from_be_bytes(entry_count_bytes);

				// read entries ({chunk_offset: u32})
				let mut entries = Vec::new();
				let mut entry_bytes = [0u8; 4];
				while let Ok(_) = reader.read_exact(&mut entry_bytes) {
					entries.push(entry_bytes);
				}

				if entry_count as usize != entries.len() {
					log::warn!("correcting stco table length: metadata says {} entries, but should actually be {} entries", entry_count, entries.len());
				}

				data.write(&(entries.len() as u32).to_be_bytes())?;
				for entry in entries {
					data.write(&entry)?;
				}

				self.found_stco = true;
			},


			_ => {
				io::copy(reader, &mut data)?;
			},
		}

		current_box.data = BoxData::Raw(data);

		Ok(())
	}

	fn end_box(&mut self, _typ: &mp4::BoxType) -> io::Result<()> {
		if let Some(exit_box) = self.stack.pop() {
			if let Some(parent_box) = self.stack.last_mut() {
				// "write" data to parent box
				match &mut parent_box.data {
					BoxData::Empty => {
						parent_box.data = BoxData::Children(vec![exit_box]);
					},
					BoxData::Raw(_) => {
						log::warn!("overriding raw data in parent with child node");
						parent_box.data = BoxData::Children(vec![exit_box]);
					},
					BoxData::Children(children) => {
						children.push(exit_box)
					}
				}
			} else {
				// TODO: move writing step to be part of recover function above
				//       identify where mdat_offset is

				// exiting root; write data to file
				exit_box.write_to(&mut self.writer)?;

				// first verify that we have all required boxes; if any are missing, append them to the file
				if !self.found_stco {
					log::warn!("stco box not found");
					if let Some(offset) = self.mdat_offset {
						log::warn!("synthesizing stco box to reference start of mdat");
						let stco_box = Mp4Box {
							header: BoxHeader {
								name: BoxType::StcoBox,
								size: 8,
							},
							data: BoxData::Raw({
								let mut stco_data = vec![
									0x00, // version
									0x00, 0x00, 0x00, // flags
									0x01, // entry count
								];
								stco_data.write(&offset.to_be_bytes())?;
								stco_data
							}),
						};

						stco_box.write_to(&mut self.writer)?;
					}
				}
			}
		}

		Ok(())
	}
}
