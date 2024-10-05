mod inspect;
mod mangle;
mod mp4;
mod strip;

use std::fs::File;
use std::io;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use clap_verbosity_flag::Verbosity;
use env_logger::{Env, Builder};
use ::mp4::{BoxType, Mp4Reader};

use crate::inspect::{ExtractVisitor, InspectVisitor, PathVisitor};
use crate::mp4::read_box;

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
	Ftyp,
	Mvhd,
	Mfhd,
	Free,
	Mdat,
	Moov,
	Mvex,
	Mehd,
	Trex,
	Emsg,
	Moof,
	Tkhd,
	Tfhd,
	Tfdt,
	Edts,
	Mdia,
	Elst,
	Mdhd,
	Hdlr,
	Minf,
	Vmhd,
	Stbl,
	Stsd,
	Stts,
	Ctts,
	Stss,
	Stsc,
	Stsz,
	Stco,
	Co64,
	Trak,
	Traf,
	Trun,
	Udta,
	Meta,
	Dinf,
	Dref,
	Url,
	Smhd,
	Avc1,
	AvcC,
	Hev1,
	HvcC,
	Mp4a,
	Esds,
	Tx3g,
	Vpcc,
	Vp09,
	Data,
	Ilst,
	Name,
	Day,
	Covr,
	Desc,
	Wide,
}

impl Into<BoxType> for BoxTypeArg {
	fn into(self) -> BoxType {
		match self {
			Self::Ftyp => BoxType::FtypBox,
			Self::Mvhd => BoxType::MvhdBox,
			Self::Mfhd => BoxType::MfhdBox,
			Self::Free => BoxType::FreeBox,
			Self::Mdat => BoxType::MdatBox,
			Self::Moov => BoxType::MoovBox,
			Self::Mvex => BoxType::MvexBox,
			Self::Mehd => BoxType::MehdBox,
			Self::Trex => BoxType::TrexBox,
			Self::Emsg => BoxType::EmsgBox,
			Self::Moof => BoxType::MoofBox,
			Self::Tkhd => BoxType::TkhdBox,
			Self::Tfhd => BoxType::TfhdBox,
			Self::Tfdt => BoxType::TfdtBox,
			Self::Edts => BoxType::EdtsBox,
			Self::Mdia => BoxType::MdiaBox,
			Self::Elst => BoxType::ElstBox,
			Self::Mdhd => BoxType::MdhdBox,
			Self::Hdlr => BoxType::HdlrBox,
			Self::Minf => BoxType::MinfBox,
			Self::Vmhd => BoxType::VmhdBox,
			Self::Stbl => BoxType::StblBox,
			Self::Stsd => BoxType::StsdBox,
			Self::Stts => BoxType::SttsBox,
			Self::Ctts => BoxType::CttsBox,
			Self::Stss => BoxType::StssBox,
			Self::Stsc => BoxType::StscBox,
			Self::Stsz => BoxType::StszBox,
			Self::Stco => BoxType::StcoBox,
			Self::Co64 => BoxType::Co64Box,
			Self::Trak => BoxType::TrakBox,
			Self::Traf => BoxType::TrafBox,
			Self::Trun => BoxType::TrunBox,
			Self::Udta => BoxType::UdtaBox,
			Self::Meta => BoxType::MetaBox,
			Self::Dinf => BoxType::DinfBox,
			Self::Dref => BoxType::DrefBox,
			Self::Url => BoxType::UrlBox,
			Self::Smhd => BoxType::SmhdBox,
			Self::Avc1 => BoxType::Avc1Box,
			Self::AvcC => BoxType::AvcCBox,
			Self::Hev1 => BoxType::Hev1Box,
			Self::HvcC => BoxType::HvcCBox,
			Self::Mp4a => BoxType::Mp4aBox,
			Self::Esds => BoxType::EsdsBox,
			Self::Tx3g => BoxType::Tx3gBox,
			Self::Vpcc => BoxType::VpccBox,
			Self::Vp09 => BoxType::Vp09Box,
			Self::Data => BoxType::DataBox,
			Self::Ilst => BoxType::IlstBox,
			Self::Name => BoxType::NameBox,
			Self::Day => BoxType::DayBox,
			Self::Covr => BoxType::CovrBox,
			Self::Desc => BoxType::DescBox,
			Self::Wide => BoxType::WideBox,
		}
	}
}

#[derive(Subcommand)]
enum Command {
	/// Attempt to read box/atom structure from the given MP4 file outright
	Inspect {
		/// path to target file
		file: PathBuf,

		/// Perform deep inspection on the given MP4 file (only works with a mostly-well-formed MP4 file)
		#[arg(long)]
		deep: bool,

		/// Print paths instead of an indented tree (mutually exclusive with --deep)
		#[arg(long)]
		paths: bool,
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

	/// Strips the specified boxes/atoms from a video file
	Strip {
		/// ignore the following types of boxes/atoms
		#[arg(short='x', long)]
		ignore: Vec<BoxTypeArg>,

		/// path to input video file
		input: PathBuf,
		/// path to target output file
		output: PathBuf,
	},
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
		Command::Inspect { file, deep, paths } => {
			let f = File::open(file)?;
			let size = f.metadata()?.len();
			let reader = io::BufReader::new(f);

			if deep {
				let mp4 = Mp4Reader::read_header(reader, size)?;
				println!("{:#?}", mp4);
			} else if paths {
				let mut visitor = PathVisitor::default();
				read_box(reader, size, &mut visitor)?;
			} else {
				let mut visitor = InspectVisitor::default();
				read_box(reader, size, &mut visitor)?;
			}
		},

		Command::Extract { box_type, input, output } => {
			let in_file = File::open(input)?;
			let in_file_size = in_file.metadata()?.len();
			let reader = io::BufReader::new(in_file);

			let out_file = File::create(output)?;
			let mut writer = io::BufWriter::new(out_file);

			let mut visitor = ExtractVisitor::new(box_type.into(), &mut writer);
			read_box(reader, in_file_size, &mut visitor)?;
		},

		Command::Mangle(mangle_command) => match mangle_command {
			MangleCommand::Flip { count, file } => mangle::flip_bits(&file, count)?,
			MangleCommand::Blank { count, block_size, file } => mangle::blank_blocks(&file, count, block_size)?,
			MangleCommand::Truncate { amount, file } => mangle::truncate(&file, amount)?,
		}

		Command::Strip { ignore, input, output } => strip::strip(&input, &output, ignore.into_iter().map(|x| x.into()).collect())?,
	}

	Ok(())
}
