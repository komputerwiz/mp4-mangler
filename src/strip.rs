use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use mp4::BoxType;

use crate::mp4::{BoxHeader, read_box, Mp4Visitor};

pub fn strip(input: &Path, output: &Path, ignore: Vec<BoxType>) -> io::Result<()> {
	let in_file = File::open(input)?;
	let in_file_size = in_file.metadata()?.len();
	let reader = io::BufReader::new(in_file);

	let out_file = File::create(output)?;
	let mut writer = io::BufWriter::new(out_file);

	let mut visitor = StripVisitor::new(&mut writer, ignore);
	read_box(reader, in_file_size, &mut visitor)?;

	Ok(())
}

struct Mp4Box {
	name: BoxType,
	data: BoxData,
	force_longsize: bool,
}

impl Mp4Box {
	fn write_to(&self, writer: &mut impl io::Write) -> io::Result<u64> {
		let mut data_buf = Vec::new();
		self.data.write_to(&mut data_buf)?;

		let mut size = 8 + data_buf.len() as u64;
		if self.force_longsize || size > u32::MAX as u64 {
			size += 8;
		}

		let name_id: u32 = self.name.into();

		if self.force_longsize || size > u32::MAX as u64 {
			writer.write_all(&1u32.to_be_bytes())?;
			writer.write_all(&name_id.to_be_bytes())?;
			writer.write_all(&size.to_be_bytes())?;
		} else {
			writer.write_all(&(size as u32).to_be_bytes())?;
			writer.write_all(&name_id.to_be_bytes())?;
		}

		writer.write_all(&data_buf)?;

		Ok(size)
	}
}

enum BoxData {
	Empty,
	Raw(Vec<u8>),
	Children(Vec<Mp4Box>),
}

impl BoxData {
	fn write_to(&self, writer: &mut impl io::Write) -> io::Result<u64> {
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

struct StripVisitor<'a> {
	writer: &'a mut dyn io::Write,
	ignore: Vec<BoxType>,
	stack: Vec<Mp4Box>,
}

impl<'a> StripVisitor<'a> {
	fn new(writer: &'a mut impl io::Write, ignore: Vec<BoxType>) -> Self {
		Self {
			writer,
			ignore,
			stack: Vec::new(),
		}
	}

}

impl<'a> Mp4Visitor for StripVisitor<'a> {
	fn start_box(&mut self, header: &BoxHeader, corrected_size: Option<u64>) -> io::Result<()> {
		if let Some(size) = corrected_size {
			log::warn!("Correcting size mismatch in {} box: header says {} B, but should actually be {} B", header.name, header.size, size);
		}

		// NOTE: maintain context for all box types; the "ignore" step will happen later.
		self.stack.push(Mp4Box {
			name: header.name,
			data: BoxData::Empty,
			force_longsize: header.longsize,
		});

		Ok(())
	}

	// Attempt to do some recovery in the form of box offset/size consistency adjustments
	fn data(&mut self, reader: &mut impl io::Read) -> io::Result<()> {
		let current_box = self.stack.last_mut().unwrap();

		let mut data: Vec<u8> = Vec::new();

		// blank ignored boxes by changing type to `free` and setting data to `[0; size]`
		if self.ignore.contains(&current_box.name) {
			log::info!("skipping data for ignored box type {}", current_box.name);
			current_box.name = BoxType::FreeBox;
			let _size = reader.read_to_end(&mut data)?;
			data.fill(0);
			current_box.data = BoxData::Raw(data);
			return Ok(());
		}

		match current_box.name {
			BoxType::CttsBox => {
				let mut version_flags = [0u8; 4];
				// version (1 B) and flags (3 B)
				reader.read_exact(&mut version_flags)?;

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

				data.write(&version_flags)?;
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

				data.write(&version_flags)?;
				data.write(&(entries.len() as u32).to_be_bytes())?;
				for entry in entries {
					data.write(&entry)?;
				}
			},

			BoxType::StscBox => {
				let mut version_flags = [0u8; 4];
				// version (1 B) and flags (3 B)
				reader.read_exact(&mut version_flags)?;

				// entry count (u32)
				let mut entry_count_bytes = [0u8; 4];
				reader.read_exact(&mut entry_count_bytes)?;
				let entry_count = u32::from_be_bytes(entry_count_bytes);

				// read entries ({first_chunk: u32, samples_per_chunk: u32, sample_description_id: u32})
				let mut entries = Vec::new();
				let mut entry_bytes = [0u8; 12];
				while let Ok(_) = reader.read_exact(&mut entry_bytes) {
					entries.push(entry_bytes);
				}

				if entry_count as usize != entries.len() {
					log::warn!("correcting stsc table length: metadata says {} entries, but should actually be {} entries", entry_count, entries.len());
				}

				data.write(&version_flags)?;
				data.write(&(entries.len() as u32).to_be_bytes())?;
				for entry in entries {
					data.write(&entry)?;
				}
			},

			BoxType::SttsBox => {
				let mut version_flags = [0u8; 4];
				// version (1 B) and flags (3 B)
				reader.read_exact(&mut version_flags)?;

				// entry count (u32)
				let mut entry_count_bytes = [0u8; 4];
				reader.read_exact(&mut entry_count_bytes)?;
				let entry_count = u32::from_be_bytes(entry_count_bytes);

				// read entries ({sample_count: u32, sample_duration: u32})
				let mut entries = Vec::new();
				let mut entry_bytes = [0u8; 8];
				while let Ok(_) = reader.read_exact(&mut entry_bytes) {
					entries.push(entry_bytes);
				}

				if entry_count as usize != entries.len() {
					log::warn!("correcting stts table length: metadata says {} entries, but should actually be {} entries", entry_count, entries.len());
				}

				data.write(&version_flags)?;
				data.write(&(entries.len() as u32).to_be_bytes())?;
				for entry in entries {
					data.write(&entry)?;
				}
			},

			// copy all other box types verbatim
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

				/*
				// verify that we have all required boxes; if any are missing, append them to the file
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
				*/
			}
		}

		Ok(())
	}
}
