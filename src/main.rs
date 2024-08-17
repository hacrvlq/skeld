mod add_subcommand;
mod launch_subcommand;
mod parse;
mod paths;
mod project;
mod sandbox;

use std::{error::Error, path::PathBuf, process::ExitCode};

use clap::Parser as _;

use launch_subcommand::{tui, CommandData};
use parse::ParseContext;

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
	match try_main() {
		Ok(code) => code,
		Err(err) => {
			eprintln!("{err}");
			ExitCode::FAILURE
		}
	}
}
fn try_main() -> Result<ExitCode, Box<dyn Error>> {
	let args = CliArgs::parse();

	let mut parse_ctx = ParseContext::new();
	// convenience macro, as config errors are displayed via 'parse_ctx'
	macro_rules! unwrap_config_error {
		($err:expr) => {
			match $err {
				Ok(val) => val,
				Err(err) => {
					parse_ctx.print_error(&err);
					return Ok(ExitCode::FAILURE);
				}
			}
		};
	}

	let config = unwrap_config_error!(parse_ctx.get_global_config());

	let subcommand = args.subcommand.unwrap_or(CliSubcommands::Launch);
	match subcommand {
		CliSubcommands::Launch => Ok(unwrap_config_error!(launch_subcommand::run(
			&mut parse_ctx,
			config
		))),
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
