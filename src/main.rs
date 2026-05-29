mod add_subcommand;
mod command;
#[path = "utils/dirs.rs"]
mod dirs;
#[path = "utils/error.rs"]
mod error;
mod parsing;
mod project;
mod sandbox;
mod ui_subcommand;
#[path = "utils/vec_ext.rs"]
mod vec_ext;

use std::{io::Read as _, path::PathBuf, process::ExitCode};

use clap::Parser as _;
use nix::{sys::termios, unistd};

use crate::parsing::ParseContext;

pub use error::{GenericError, GenericResult};

#[derive(clap::Parser)]
#[command(version, about = "Open projects in a restricted sandbox")]
struct CliArgs {
	/// Path to the config file to use
	#[arg(long = "config", id = "FILE")]
	config_file_path: Option<PathBuf>,

	#[command(subcommand)]
	subcommand: CliSubcommands,
}
#[derive(clap::Subcommand)]
enum CliSubcommands {
	/// Open the skeld tui
	Ui,
	/// Add a project
	Add(AddArgs),
}

#[derive(clap::Parser)]
struct AddArgs {
	#[arg(id = "PATH")]
	/// Path to the project
	project_path: PathBuf,
	#[arg(long = "name", id = "NAME")]
	/// Use this name instead of the name derived from the path
	project_name: Option<String>,
}

fn main() -> ExitCode {
	let mut file_database = parsing::FileDatabase::new();

	match try_main(&mut file_database) {
		Ok(code) => code,
		Err(err) => {
			err.print(&file_database);

			if is_session_leader_of_tty() {
				eprint!("Press any key to continue...");
				wait_on_stdin_input();
			}

			ExitCode::FAILURE
		}
	}
}
fn try_main(file_database: &mut parsing::FileDatabase) -> GenericResult<ExitCode> {
	let args = CliArgs::parse();

	let mut parse_ctx = ParseContext { file_database };
	let config = parse_ctx.get_global_config(args.config_file_path)?;

	match args.subcommand {
		CliSubcommands::Ui => ui_subcommand::run(&mut parse_ctx, config),
		CliSubcommands::Add(args) => {
			add_subcommand::run(args)?;
			Ok(ExitCode::SUCCESS)
		}
	}
}

fn is_session_leader_of_tty() -> bool {
	let pid = unistd::getpid();
	let tty_sid = termios::tcgetsid(std::io::stderr());
	tty_sid == Ok(pid)
}
fn wait_on_stdin_input() {
	let mut fd = std::io::stdin();

	let Ok(mut tios) = termios::tcgetattr(&fd) else {
		return;
	};

	tios.local_flags.remove(termios::LocalFlags::ICANON);
	tios.local_flags.remove(termios::LocalFlags::ECHO);
	match termios::tcsetattr(&fd, termios::SetArg::TCSANOW, &tios) {
		Ok(()) => (),
		Err(_) => return,
	}

	let _ = fd.read_exact(&mut [0]);
}
