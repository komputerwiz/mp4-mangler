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
	fn start_box(&mut self, header: &BoxHeader, _reader: &mut impl io::Read) -> io::Result<()> {
		println!("{:indent$}{} ({} B)", "", header.name, header.size, indent=self.depth * 2);
		self.depth += 1;
		Ok(())
	}

	fn end_box(&mut self, _typ: &BoxType) {
		self.depth -= 1;
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
}

impl<'a> ExtractVisitor<'a> {
	fn new(box_type: BoxType, writer: &'a mut impl io::Write) -> Self {
		Self {
			box_type,
			writer,
		}
	}
}

impl<'a> Mp4Visitor for ExtractVisitor<'a> {
	fn start_box(&mut self, header: &BoxHeader, reader: &mut impl io::Read) -> io::Result<()> {
		if header.name == self.box_type {
			io::copy(reader, self.writer)?;
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
