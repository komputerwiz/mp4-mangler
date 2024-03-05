use std::fs::File;
use std::io;
use std::path::Path;

use crate::mp4::{Mp4Visitor, read_box};
use mp4::{BoxHeader, BoxType};

#[derive(Default)]
struct InspectVisitor {
	depth: usize,
}

impl Mp4Visitor for InspectVisitor {
	fn start_box(&mut self, header: &BoxHeader, corrected_size: Option<u64>) -> io::Result<()> {
		if let Some(size) = corrected_size {
			println!("{:indent$}{} ({} B declared, {} B corrected)", "", header.name, header.size, size, indent=self.depth * 2);
		} else {
			println!("{:indent$}{} ({} B)", "", header.name, header.size, indent=self.depth * 2);
		}
		self.depth += 1;

		Ok(())
	}

	fn end_box(&mut self, _typ: &BoxType) -> io::Result<()> {
		self.depth -= 1;

		Ok(())
	}
}

pub fn inspect(file: &Path) -> io::Result<()> {
	let f = File::open(file)?;
	let file_size = f.metadata()?.len();
	let reader = io::BufReader::new(f);

	let mut visitor = InspectVisitor::default();
	read_box(reader, file_size, &mut visitor)?;

	Ok(())
}


struct ExtractVisitor<'a> {
	box_type: BoxType,
	writer: &'a mut dyn io::Write,
	should_read: bool,
}

impl<'a> ExtractVisitor<'a> {
	fn new(box_type: BoxType, writer: &'a mut impl io::Write) -> Self {
		Self {
			box_type,
			writer,
			should_read: false,
		}
	}
}

impl<'a> Mp4Visitor for ExtractVisitor<'a> {
	fn start_box(&mut self, header: &BoxHeader, _corrected_size: Option<u64>) -> io::Result<()> {
		if header.name == self.box_type {
			self.should_read = true;
		}

		Ok(())
	}

	fn data(&mut self, reader: &mut impl io::Read) -> io::Result<()> {
		if self.should_read {
			io::copy(reader, self.writer)?;
			self.should_read = false;
		}

		Ok(())
	}
}

pub fn extract(box_type: BoxType, input: &Path, output: &Path) -> io::Result<()> {
	let in_file = File::open(input)?;
	let in_file_size = in_file.metadata()?.len();
	let reader = io::BufReader::new(in_file);

	let out_file = File::create(output)?;
	let mut writer = io::BufWriter::new(out_file);

	let mut visitor = ExtractVisitor::new(box_type, &mut writer);
	read_box(reader, in_file_size, &mut visitor)?;

	Ok(())
}
