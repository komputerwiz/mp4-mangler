use std::io;

use crate::mp4::{BoxHeader, BoxType, Mp4Visitor};

#[derive(Default)]
pub struct PathVisitor {
	path: Vec<String>,
	with_size: bool,
}

impl PathVisitor {
	pub fn new(with_size: bool) -> Self {
		Self {
			with_size,
			..Default::default()
		}
	}
}

impl Mp4Visitor for PathVisitor {
	fn start_box(&mut self, header: &BoxHeader, corrected_size: Option<u64>) -> io::Result<()> {
		let size_description = if self.with_size {
			if let Some(actual_size) = corrected_size {
				format!(" ({} B declared, {} B corrected)", header.size, actual_size)
			} else {
				format!(" ({} B)", header.size)
			}
		} else {
			"".into()
		};

		self.path.push(header.name.to_string());
		println!("{}{}", self.path.join("/"), size_description);
		Ok(())
  }

  fn end_box(&mut self, _typ: &BoxType) -> io::Result<()> {
    self.path.pop();
    Ok(())
  }
}

#[derive(Default)]
pub struct PrintTreeVisitor {
	depth: usize,
	with_size: bool,
}

impl PrintTreeVisitor {
	pub fn new(with_size: bool) -> Self {
		Self {
			with_size,
			..Default::default()
		}
	}
}

impl Mp4Visitor for PrintTreeVisitor {
	fn start_box(&mut self, header: &BoxHeader, corrected_size: Option<u64>) -> io::Result<()> {
		let size_description = if self.with_size {
			if let Some(actual_size) = corrected_size {
				format!(" ({} B declared, {} B corrected)", header.size, actual_size)
			} else {
				format!(" ({} B)", header.size)
			}
		} else {
			"".into()
		};
		println!("{:indent$}{}{}", "", header.name, size_description, indent=self.depth * 2);
		self.depth += 1;

		Ok(())
	}

	fn end_box(&mut self, _typ: &BoxType) -> io::Result<()> {
		self.depth -= 1;

		Ok(())
	}
}

pub struct ExtractVisitor<'a> {
	box_type: BoxType,
	writer: &'a mut dyn io::Write,
	should_read: bool,
}

impl<'a> ExtractVisitor<'a> {
	pub fn new(box_type: BoxType, writer: &'a mut impl io::Write) -> Self {
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
