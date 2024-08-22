mod add_subcommand;
mod error;
mod launch_subcommand;
mod parse;
mod paths;
mod project;
mod sandbox;

use std::{path::PathBuf, process::ExitCode};

use clap::Parser as _;

use crate::{
	launch_subcommand::{tui, CommandData},
	parse::ParseContext,
};

pub use error::{GenericError, GenericResult};

#[derive(clap::Parser)]
#[command(version, about = "Open projects in a restricted sandbox")]
struct CliArgs {
	#[command(subcommand)]
	subcommand: Option<CliSubcommands>,
}
#[derive(clap::Subcommand)]
enum CliSubcommands {
	/// Launch the skeld tui (Default Command)
	Launch,
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
	let mut file_database = parse::FileDatabase::new();

	match try_main(&mut file_database) {
		Ok(code) => code,
		Err(err) => {
			err.print(&file_database);
			ExitCode::FAILURE
		}
	}
}
fn try_main(file_database: &mut parse::FileDatabase) -> GenericResult<ExitCode> {
	let args = CliArgs::parse();

	let mut parse_ctx = ParseContext { file_database };
	let config = parse_ctx.get_global_config()?;

	let subcommand = args.subcommand.unwrap_or(CliSubcommands::Launch);
	match subcommand {
		CliSubcommands::Launch => launch_subcommand::run(&mut parse_ctx, config),
		CliSubcommands::Add(args) => {
			add_subcommand::run(args)?;
			Ok(ExitCode::SUCCESS)
		}
	}
}

pub struct GlobalConfig {
	pub banner: String,
	pub colorscheme: tui::Colorscheme,
	pub commands: Vec<CommandData>,
	pub global_project_data: parse::PrelimParseState,
}
