mod inspect;
mod mangle;
mod mp4;

use std::fs::File;
use std::io;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use clap_verbosity_flag::Verbosity;
use env_logger::{Env, Builder};
use ::mp4::{BoxType, Mp4Reader};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
	#[command(flatten)]
	verbose: Verbosity,

	#[command(subcommand)]
	command: Command,
}

#[derive(Clone, ValueEnum)]
enum BoxTypeArg {
	Mdat,
	Moov,
}

impl Into<BoxType> for BoxTypeArg {
	fn into(self) -> BoxType {
		match self {
			BoxTypeArg::Mdat => BoxType::MdatBox,
			BoxTypeArg::Moov => BoxType::MoovBox,
		}
	}
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

	/// Extract a specific box payload from the given MP4 file
	Extract {
		/// type of box/atom to extract
		#[arg(value_enum)]
		box_type: BoxTypeArg,
		/// path to input video file
		input: PathBuf,
		/// path to target output file
		output: PathBuf,
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

			let mp4 = Mp4Reader::read_header(reader, size)?;
			println!("{:#?}", mp4);
		},

		Command::Inspect { file } => inspect::inspect(&file)?,

		Command::Extract { box_type, input, output } => inspect::extract(box_type.into(), &input, &output)?,

		Command::Mangle(mangle_command) => match mangle_command {
			MangleCommand::Flip { count, file } => mangle::flip_bits(&file, count)?,
			MangleCommand::Blank { count, block_size, file } => mangle::blank_blocks(&file, count, block_size)?,
			MangleCommand::Truncate { amount, file } => mangle::truncate(&file, amount)?,
		}
	}

	Ok(())
}
