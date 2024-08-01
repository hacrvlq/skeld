mod action;
mod parse;
mod project;
mod sandbox;
mod tui;

use std::{error::Error, process::ExitCode};

use action::Action;
use parse::ParseContext;
use tui::{TuiData, UserSelection};

// TODO: cli arguments
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

	let commands = config.commands.clone().into_iter().map(|data| tui::Button {
		keybind: data.keybind,
		text: data.name,
		action: Action::Run(data.command),
	});

	let bookmarks = unwrap_config_error!(parse_ctx.get_bookmarks())
		.into_iter()
		.map(|data| tui::Button {
			keybind: data.keybind,
			text: data.name,
			action: Action::OpenProject(data.project_data),
		});

	let projects = unwrap_config_error!(parse_ctx.get_projects())
		.into_iter()
		.enumerate()
		.map(|(i, data)| tui::Button {
			keybind: i.to_string(),
			text: data.name,
			action: Action::OpenProject(data.project_data),
		});

	let sections = [
		tui::Section {
			heading: "Commands".to_string(),
			buttons: commands.collect(),
		},
		tui::Section {
			heading: "Bookmarks".to_string(),
			buttons: bookmarks.collect(),
		},
		tui::Section {
			heading: "Projects".to_string(),
			buttons: projects.collect(),
		},
	]
	.into_iter()
	.filter(|section| !section.buttons.is_empty());

	let tui_data = TuiData {
		banner: config.banner.clone(),
		colorscheme: config.colorscheme.clone(),
		sections: sections.collect(),
	};

	let action = tui::run(&tui_data)?;
	match action {
		UserSelection::ControlC => Ok(ExitCode::SUCCESS),
		UserSelection::Button(action) => Ok(unwrap_config_error!(
			action.execute(config.global_project_data, &mut parse_ctx)
		)),
	}
}
