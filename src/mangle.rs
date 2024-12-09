use std::fmt;
use std::fs::OpenOptions;
use std::io;
use std::os::unix::prelude::FileExt;
use std::path::Path;

use rand::{self, distributions::Uniform, prelude::Distribution};

pub enum Amount {
	Percent(f64),
	Count(u64),
}

#[derive(Debug)]
pub enum AmountError {
	MissingValue,
	PercentRange,
}

impl std::error::Error for AmountError {}

impl fmt::Display for AmountError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			AmountError::MissingValue => write!(f, "no value provided for amount"),
			AmountError::PercentRange => write!(f, "percent range should be between 0 and 1"),
		}
	}
}

impl TryFrom<(Option<f64>, Option<u64>)> for Amount {
	type Error = AmountError;
	fn try_from(value: (Option<f64>, Option<u64>)) -> Result<Self, Self::Error> {
		match value {
			(Some(pct), _) => if pct < 0.0 || pct > 1.0 { Err(AmountError::PercentRange) } else { Ok(Self::Percent(pct)) },
			(_, Some(ct)) => Ok(Self::Count(ct)),
			_ => Err(AmountError::MissingValue),
		}
	}
}

impl TryFrom<(Option<u64>, Option<f64>)> for Amount {
	type Error = AmountError;
	fn try_from(value: (Option<u64>, Option<f64>)) -> Result<Self, Self::Error> {
		match value {
			(Some(ct), _) => Ok(Self::Count(ct)),
			(_, Some(pct)) => if pct < 0.0 || pct > 1.0 { Err(AmountError::PercentRange) } else { Ok(Self::Percent(pct)) },
			_ => Err(AmountError::MissingValue),
		}
	}
}

/// flips random bits in the given file
pub fn flip_bits(file: &Path, bits_amount: Amount) -> io::Result<()> {
	log::info!("opening file {}", file.to_str().unwrap_or("???"));
	let f = OpenOptions::new().read(true).write(true).open(file)?;
	let file_size_bytes = f.metadata()?.len();

	let mut rng = rand::thread_rng();
	let size_dist = Uniform::new(0, file_size_bytes);
	let bit_dist = Uniform::new(0, 8);

	let num_bits = match bits_amount {
		Amount::Count(x) => x,
		Amount::Percent(pct) => (pct * file_size_bytes as f64 * 8.0).ceil() as u64,
	};

	for _ in 0..num_bits {
		// read byte at random byte offset
		let byte_offset = size_dist.sample(&mut rng);
		let mut buf: [u8; 1] = Default::default();
		f.read_exact_at(&mut buf, byte_offset)?;

		let orig = buf[0];

		// flip bit at random bit offset
		let bit_offset = bit_dist.sample(&mut rng);
		buf[0] ^= 1 << bit_offset;
		log::debug!("Flipped bit {} at offset 0x{:x}: 0x{:02x} => 0x{:02x}", 8 - bit_offset, byte_offset, orig, buf[0]);

		// write byte back to file
		f.write_at(&buf, byte_offset)?;
	}

	log::info!("Flipped {} bit{}", num_bits, if num_bits == 1 { "" } else { "s" });

	Ok(())
}

/// zeroes random contiguous blocks of data
pub fn blank_blocks(file: &Path, blocks_amount: Amount, block_size_bytes: u64) -> io::Result<()> {
	log::info!("opening file {}", file.to_str().unwrap_or("???"));
	let f = OpenOptions::new().read(true).write(true).open(file)?;
	let file_size_bytes = f.metadata()?.len();

	let bad_block = vec![0; block_size_bytes as usize];
	let file_size_blocks = file_size_bytes / block_size_bytes;

	let mut rng = rand::thread_rng();
	let block_dist = Uniform::new(0, file_size_blocks);

	let num_blocks = match blocks_amount {
		Amount::Count(x) => x,
		Amount::Percent(pct) => (pct * file_size_blocks as f64).ceil() as u64,
	};

	for _ in 0..num_blocks {
		let block_offset = block_dist.sample(&mut rng);

		let end = if block_offset == file_size_blocks - 1 {
			// don't go beyond end of last block
			file_size_bytes % block_size_bytes
		} else {
			block_size_bytes
		} as usize;

		log::debug!("Blanked block {} ({} bytes starting at offset 0x{:x})", block_offset, block_size_bytes, block_offset * block_size_bytes);

		f.write_at(&bad_block[0..end], block_offset * block_size_bytes)?;
	}

	log::info!("Blanked {} block{}", num_blocks, if num_blocks == 1 { "" } else { "s" });

	Ok(())
}

/// truncates the given number of bytes from the end of the file.
pub fn truncate(file: &Path, bytes_amount: Amount) -> io::Result<()> {
	log::info!("opening file {}", file.to_str().unwrap_or("???"));
	let f = OpenOptions::new().read(true).write(true).open(file)?;
	let file_size_bytes = f.metadata()?.len();

	let num_bytes = match bytes_amount {
		Amount::Count(x) => x,
		Amount::Percent(pct) => (pct * file_size_bytes as f64).ceil() as u64,
	};

	f.set_len(file_size_bytes - num_bytes)?;

	log::info!("Truncated {} byte{}", num_bytes, if num_bytes == 1 { "" } else { "s" });

	Ok(())
}
