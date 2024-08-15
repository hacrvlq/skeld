mod launch_subcommand;
mod parse;
mod project;
mod sandbox;

use std::{error::Error, process::ExitCode};

use launch_subcommand::{tui, CommandData};
use parse::ParseContext;

fn main() -> ExitCode {
	clap::command!()
		.name("Skeld")
		.about("Open projects in a restricted sandbox")
		.get_matches();

	match try_main() {
		Ok(code) => code,
		Err(err) => {
			eprintln!("{err}");
			ExitCode::FAILURE
		}
	}
}
fn try_main() -> Result<ExitCode, Box<dyn Error>> {
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
	Ok(unwrap_config_error!(launch_subcommand::run(
		&mut parse_ctx,
		config
	)))
}

pub struct GlobalConfig {
	pub banner: String,
	pub colorscheme: tui::Colorscheme,
	pub commands: Vec<CommandData>,
	pub global_project_data: parse::PrelimParseState,
}
