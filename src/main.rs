mod inspect;
mod mangle;

use std::fs::File;
use std::io::{self, Seek};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use clap_verbosity_flag::Verbosity;
use env_logger::{Env, Builder};


#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
	#[command(flatten)]
	verbose: Verbosity,

	#[command(subcommand)]
	command: Command,
}

#[derive(Subcommand)]
enum Command {
	/// Attempt to read box/atom structure from the given MP4 file outright
	Read {
		/// path to target file
		file: PathBuf,
	},

	/// Perform deep inspection on the given MP4 file
	Inspect {
		/// path to target file
		file: PathBuf,
	},

	/// intentionally corrupt a given file
	#[command(subcommand)]
	Mangle(MangleCommand),
}

#[derive(Subcommand)]
enum MangleCommand {
	/// flips random bits in the given file
	Flip {
		/// number of random bits to flip
		#[arg(short, long, default_value = "1")]
		count: usize,
		/// path to target file
		file: PathBuf,
	},

	/// sets a block of data to zeroes
	Blank {
		/// number of random blocks to blank out
		#[arg(short, long, default_value = "1")]
		count: usize,
		/// size of block in bytes
		#[arg(short, long, default_value = "4096")]
		block_size: u64,
		/// path to target file
		file: PathBuf,
	},

	/// truncates data from the end of the file
	Truncate {
		/// length of data to truncate in bytes
		amount: u64,
		/// path to target file
		file: PathBuf,
	}
}

fn main() -> Result<(), Box<dyn std::error::Error>>{
	let cli = Cli::parse();

	let env = Env::default()
		.filter("LOG_LEVEL")
		.write_style("LOG_STYLE");

	Builder::from_env(env)
		.filter_level(cli.verbose.log_level_filter())
		.format_timestamp_secs()
		.init();

	log::trace!("logger initialized");

	match cli.command {
		Command::Read { file } => {
			let f = File::open(file)?;
			let size = f.metadata()?.len();
			let reader = io::BufReader::new(f);

			let mp4 = mp4::Mp4Reader::read_header(reader, size)?;
			println!("{:#?}", mp4);
		},

		Command::Inspect { file } => inspect::inspect(&file)?,

		Command::Mangle(mangle_command) => match mangle_command {
			MangleCommand::Flip { count, file } => mangle::flip_bits(&file, count)?,
			MangleCommand::Blank { count, block_size, file } => mangle::blank_blocks(&file, count, block_size)?,
			MangleCommand::Truncate { amount, file } => mangle::truncate(&file, amount)?,
		}
	}

	Ok(())
}
