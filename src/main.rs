mod action;
mod parse;
mod project;
mod sandbox;
mod ui;

use std::{error::Error, process::ExitCode};

use action::Action;
use parse::ParseContext;
use ui::{UiContent, UserSelection};

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

	let commands = config.commands.clone().into_iter().map(|data| ui::Button {
		keybind: data.keybind,
		text: data.name,
		action: Action::Run(data.command),
	});

	let bookmarks = unwrap_config_error!(parse_ctx.get_bookmarks())
		.into_iter()
		.map(|data| ui::Button {
			keybind: data.keybind,
			text: data.name,
			action: Action::OpenProject(data.project_data),
		});

	let projects = unwrap_config_error!(parse_ctx.get_projects())
		.into_iter()
		.enumerate()
		.map(|(i, data)| ui::Button {
			keybind: i.to_string(),
			text: data.name,
			action: Action::OpenProject(data.project_data),
		});

	let sections = [
		ui::Section {
			heading: "Commands".to_string(),
			buttons: commands.collect(),
		},
		ui::Section {
			heading: "Bookmarks".to_string(),
			buttons: bookmarks.collect(),
		},
		ui::Section {
			heading: "Projects".to_string(),
			buttons: projects.collect(),
		},
	]
	.into_iter()
	.filter(|section| !section.buttons.is_empty());

	let ui_content = UiContent {
		banner: config.banner.clone(),
		colorscheme: config.colorscheme.clone(),
		sections: sections.collect(),
	};

	let action = ui::start(&ui_content)?;
	match action {
		UserSelection::ControlC => Ok(ExitCode::SUCCESS),
		UserSelection::Button(action) => Ok(unwrap_config_error!(
			action.execute(config.global_config_data, &mut parse_ctx)
		)),
	}
}
