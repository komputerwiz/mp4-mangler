use std::io;

use crate::mp4::{BoxData, BoxHeader, BoxType, Mp4Box, Mp4Visitor};

#[derive(Default)]
pub struct MoovLocatorVisitor {
	stack: Vec<Mp4Box>,
	extracting_moov: bool,
	pub moov: Option<Mp4Box>,
}

impl MoovLocatorVisitor {
	fn current_path(&self) -> String {
		self.stack.iter()
			.map(|b| format!("{}", b.name))
			.collect::<Vec<String>>()
			.join("/")
	}
}

impl Mp4Visitor for MoovLocatorVisitor {
	fn start_box(&mut self, header: &BoxHeader, corrected_size: Option<u64>) -> io::Result<()> {
		log::trace!("start_box {}/{}", self.current_path(), header.name);

		if let Some(size) = corrected_size {
			log::warn!("Correcting size mismatch in {} box: header says {} B, but should actually be {} B", header.name, header.size, size);
		}

		if header.name == BoxType::MoovBox {
			self.extracting_moov = true;
		}

		self.stack.push(Mp4Box {
			name: header.name,
			data: BoxData::Empty,
			force_longsize: header.longsize,
		});

		Ok(())
	}

	fn data(&mut self, reader: &mut impl io::Read) -> io::Result<()> {
		log::trace!("data {}", self.current_path());
		if self.extracting_moov {
			let current_box = self.stack.last_mut().unwrap();

			let mut data: Vec<u8> = Vec::new();
			io::copy(reader, &mut data)?;
			current_box.data = BoxData::Raw(data);
		}

		Ok(())
	}

	fn end_box(&mut self, typ: &BoxType) -> io::Result<()> {
		log::trace!("end_box {}", self.current_path());

		if let Some(exit_box) = self.stack.pop() {
			if *typ == BoxType::MoovBox {
				self.moov = Some(exit_box);
				self.extracting_moov = false;
			} else if self.extracting_moov {
				// write moov ancestry to tree
				if let Some(parent_box) = self.stack.last_mut() {
					match &mut parent_box.data {
						BoxData::Empty => {
							log::trace!("appending first child to empty parent {} box", parent_box.name);
							parent_box.data = BoxData::Children(vec![exit_box]);
						},
						BoxData::Raw(_) => {
							log::warn!("overriding raw data in parent with child node");
							parent_box.data = BoxData::Children(vec![exit_box]);
						},
						BoxData::Children(children) => {
							log::trace!("appending new child to parent {} box", parent_box.name);
							children.push(exit_box)
						}
					}
				}
			}
		}

		Ok(())
	}
}

pub struct MoovTransplantVisitor<'a> {
	writer: &'a mut dyn io::Write,
	stack: Vec<Mp4Box>,

	moov_box: Mp4Box,
	replacing_moov: bool,
	found_moov: bool,
}

impl<'a> MoovTransplantVisitor<'a> {
	pub fn new(writer: &'a mut impl io::Write, moov_box: Mp4Box) -> Self {
		Self {
			writer,
			stack: Vec::new(),
			moov_box,
			replacing_moov: false,
			found_moov: false,
		}
	}

	pub fn finish(&mut self) -> io::Result<()> {
		// if moov atom not found, just append to end of file.
		if !self.found_moov {
			self.moov_box.write_to(&mut self.writer)?;
		}

		Ok(())
	}
}

impl<'a> Mp4Visitor for MoovTransplantVisitor<'a> {
	fn start_box(&mut self, header: &BoxHeader, corrected_size: Option<u64>) -> io::Result<()> {
		if let Some(size) = corrected_size {
			log::warn!("Correcting size mismatch in {} box: header says {} B, but should actually be {} B", header.name, header.size, size);
		}

		self.stack.push(if header.name == BoxType::MoovBox {
			self.found_moov = true;
			self.replacing_moov = true;

			self.moov_box.clone()
		} else {
			Mp4Box {
				name: header.name,
				data: BoxData::Empty,
				force_longsize: header.longsize,
			}
		});

		Ok(())
	}

	fn data(&mut self, reader: &mut impl io::Read) -> io::Result<()> {
		if !self.replacing_moov {
			let current_box = self.stack.last_mut().unwrap();

			let mut data: Vec<u8> = Vec::new();
			io::copy(reader, &mut data)?;
			current_box.data = BoxData::Raw(data);
		}

		Ok(())
	}

	fn end_box(&mut self, typ: &BoxType) -> io::Result<()> {
		if *typ == BoxType::MoovBox {
			self.replacing_moov = false;
		}

		if let Some(exit_box) = self.stack.pop() {
			// avoid touching moov contents
			if !self.replacing_moov {
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
					// exiting root; write data to file
					exit_box.write_to(&mut self.writer)?;
				}
			}
		}

		Ok(())
	}
}
