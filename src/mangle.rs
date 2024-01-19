use std::fs::OpenOptions;
use std::io;
use std::os::unix::prelude::FileExt;
use std::path::Path;

use rand::{self, distributions::Uniform, prelude::Distribution};

/// flips random bits in the given file
pub fn flip_bits(file: &Path, num_bits: usize) -> io::Result<()> {
	log::info!("opening file {}", file.to_str().unwrap_or("???"));
	let f = OpenOptions::new().read(true).write(true).open(file)?;
	let size = f.metadata()?.len();

	let mut rng = rand::thread_rng();
	let size_dist = Uniform::new(0, size);
	let bit_dist = Uniform::new(0, 8);

	for _ in 0..num_bits {
		// read byte at random byte offset
		let byte_offset = size_dist.sample(&mut rng);
		let mut buf: [u8; 1] = Default::default();
		f.read_exact_at(&mut buf, byte_offset)?;

		let orig = buf[0];

		// flip bit at random bit offset
		let bit_offset = bit_dist.sample(&mut rng);
		buf[0] ^= 1 << bit_offset;
		println!("Flipped bit {} at offset 0x{:x}: 0x{:02x} => 0x{:02x}", 8 - bit_offset, byte_offset, orig, buf[0]);

		// write byte back to file
		f.write_at(&buf, byte_offset)?;
	}

	Ok(())
}

/// zeroes random contiguous blocks of data
pub fn blank_blocks(file: &Path, num_blocks: usize, block_size_bytes: u64) -> io::Result<()> {
	log::info!("opening file {}", file.to_str().unwrap_or("???"));
	let f = OpenOptions::new().read(true).write(true).open(file)?;
	let size = f.metadata()?.len();

	let bad_block = vec![0; block_size_bytes as usize];
	let size_in_blocks = size / block_size_bytes;

	let mut rng = rand::thread_rng();
	let block_dist = Uniform::new(0, size_in_blocks);

	for _ in 0..num_blocks {
		let block_offset = block_dist.sample(&mut rng);

		let end = if block_offset == size_in_blocks - 1 {
			// don't go beyond end of last block
			size % block_size_bytes
		} else {
			block_size_bytes
		} as usize;

		println!("Blanked block {} ({} bytes starting at offset 0x{:x})", block_offset, block_size_bytes, block_offset * block_size_bytes);

		f.write_at(&bad_block[0..end], block_offset * block_size_bytes)?;
	}

	Ok(())
}

/// truncates the given number of bytes from the end of the file.
pub fn truncate(file: &Path, num_bytes: u64) -> io::Result<()> {
	log::info!("opening file {}", file.to_str().unwrap_or("???"));
	let f = OpenOptions::new().read(true).write(true).open(file)?;
	let size = f.metadata()?.len();

	f.set_len(size - num_bytes)?;

	println!("Truncated {} bytes from end of file.", num_bytes);

	Ok(())
}
