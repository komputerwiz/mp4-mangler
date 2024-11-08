mod inspect;
mod mangle;
mod moov_transplant;
mod mp4;
mod strip;

use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};
use std::time::{Duration, Instant};
use std::thread;

use clap::{Parser, Subcommand, ValueEnum};
use clap_verbosity_flag::Verbosity;
use env_logger::{Env, Builder};
use ::mp4::Mp4Reader;

use crate::inspect::{ExtractVisitor, PrintTreeVisitor, PathVisitor};
use crate::mp4::{BoxType, read_box};
use crate::moov_transplant::{MoovLocatorVisitor, MoovTransplantVisitor};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
	#[command(flatten)]
	verbose: Verbosity,

	#[command(subcommand)]
	command: AppCommand,
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
enum AppCommand {
	/// Attempt to read box/atom structure from the given MP4 file
	#[command(subcommand)]
	Inspect(InspectCommand),

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

	/// "Recovers" a corrupted video by splicing in a new moov box/atom contained in the specified file
	MoovTransplant {
		/// path to source file containing source moov atom
		input_moov: PathBuf,
		/// path to corrupt file
		input_subject: PathBuf,
		/// path to output file
		output: PathBuf,
	},
}

#[derive(Subcommand)]
enum InspectCommand {
	/// Tests playability using mpv
	/// Exits 0 if the file is playable or 1 if the file is unplayable
	IsPlayable {
		/// amount of time to test playability (in milliseconds)
		/// input file is considered playable if this time elapses and mpv hasn't exited
		#[arg(short = 't', long = "timeout", default_value = "300")]
		timeout_ms: u64,

		/// path to target file
		file: PathBuf,
	},

	/// Print information about the MP4 box/atom tree structure
	Tree {
		/// path to target file
		file: PathBuf,

		/// Print paths instead of an indented tree
		#[arg(long)]
		paths: bool,

		/// Print box sizes
		#[arg(long)]
		with_size: bool,
	},

	/// Perform deep inspection on the given MP4 file by parsing and printing debug-formatted output (only works with a mostly-well-formed MP4 file)
	Debug {
		/// path to target file
		file: PathBuf,
	}
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
		AppCommand::Inspect(inspect_command) => match inspect_command {
			InspectCommand::IsPlayable { timeout_ms, file } => {
				log::trace!("spawning mpv");
				let mut mpv = Command::new("mpv").arg(file)
					.stderr(if log::log_enabled!(log::Level::Debug) { Stdio::inherit() } else { Stdio::null() })
					.stdout(if log::log_enabled!(log::Level::Trace) { Stdio::inherit() } else { Stdio::null() })
					.spawn()?;

				let start_time = Instant::now();
				let poll_interval = Duration::from_millis(100.min(timeout_ms));
				let timeout = Duration::from_millis(timeout_ms);

				let is_playable = loop {
					if let Some(exit_status) = mpv.try_wait()? {
						log::trace!("mpv exited");
						// mpv exited; reflect exit status
						break exit_status.success();
					}

					if start_time.elapsed() >= timeout {
						log::trace!("killing mpv");
						// video is playing; terminate the process
						mpv.kill()?;
						break true;
					}

					thread::sleep(poll_interval);
				};

				process::exit(if is_playable {
					log::info!("file is playable");
					0
				} else {
					log::info!("file is not playable");
					1
				});
			},

			InspectCommand::Tree { file, paths, with_size } => {
				let f = File::open(file)?;
				let size = f.metadata()?.len();
				let reader = io::BufReader::new(f);

				if paths {
					let mut visitor = PathVisitor::new(with_size);
					read_box(reader, size, &mut visitor)?;
				} else {
					let mut visitor = PrintTreeVisitor::new(with_size);
					read_box(reader, size, &mut visitor)?;
				}
			},

			InspectCommand::Debug { file } => {
				let f = File::open(file)?;
				let size = f.metadata()?.len();
				let reader = io::BufReader::new(f);

				let mp4 = Mp4Reader::read_header(reader, size)?;
				println!("{:#?}", mp4);
			},
		},

		AppCommand::Extract { box_type, input, output } => {
			let in_file = File::open(input)?;
			let in_file_size = in_file.metadata()?.len();
			let reader = io::BufReader::new(in_file);

			let out_file = File::create(output)?;
			let mut writer = io::BufWriter::new(out_file);

			let mut visitor = ExtractVisitor::new(box_type.into(), &mut writer);
			read_box(reader, in_file_size, &mut visitor)?;
		},

		AppCommand::Mangle(mangle_command) => match mangle_command {
			MangleCommand::Flip { count, file } => mangle::flip_bits(&file, count)?,
			MangleCommand::Blank { count, block_size, file } => mangle::blank_blocks(&file, count, block_size)?,
			MangleCommand::Truncate { amount, file } => mangle::truncate(&file, amount)?,
		}

		AppCommand::Strip { ignore, input, output } => strip::strip(&input, &output, ignore.into_iter().map(|x| x.into()).collect())?,

		AppCommand::MoovTransplant { input_moov, input_subject, output } => {
			let moov_file = File::open(input_moov)?;
			let moov_file_size = moov_file.metadata()?.len();
			let moov_reader = io::BufReader::new(moov_file);

			let mut moov_visitor = MoovLocatorVisitor::default();
			read_box(moov_reader, moov_file_size, &mut moov_visitor)?;

			if let Some(moov_box) = moov_visitor.moov {
				let in_file = File::open(input_subject)?;
				let in_file_size = in_file.metadata()?.len();
				let reader = io::BufReader::new(in_file);

				let out_file = File::create(output)?;
				let mut writer = io::BufWriter::new(out_file);

				let mut transplant_visitor = MoovTransplantVisitor::new(&mut writer, moov_box);
				read_box(reader, in_file_size, &mut transplant_visitor)?;
				transplant_visitor.finish()?;
			} else {
				println!("Unable to find moov atom in source file");
				return Err(io::Error::other("Invalid moov source file").into());
			}
		}
	}

	Ok(())
}
