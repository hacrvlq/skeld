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

use std::{path::PathBuf, process::ExitCode};

use clap::Parser as _;

use crate::{
	parsing::ParseContext,
	ui_subcommand::{CommandData, tui},
};

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

struct GlobalConfig {
	banner: String,
	colorscheme: tui::Colorscheme,
	disable_help_text: bool,
	commands: Vec<CommandData>,
	global_project_data: parsing::PrelimParseState,
}
