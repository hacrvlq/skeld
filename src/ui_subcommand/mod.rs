pub mod tui;

use std::process::{Command as OsCommand, ExitCode};

use nix::unistd;

use self::tui::{TuiData, UserSelection};
use crate::{
	parse::{ParseContext, PrelimParseState, ProjectDataFuture},
	GenericResult,
};

pub fn run(
	parse_ctx: &mut ParseContext,
	global_config: crate::GlobalConfig,
) -> GenericResult<ExitCode> {
	let commands = global_config.commands.into_iter().map(|data| tui::Button {
		keybind: data.keybind,
		text: data.name,
		action: Action::Run(data.command),
	});

	let bookmarks = parse_ctx
		.get_bookmarks()?
		.into_iter()
		.map(|data| tui::Button {
			keybind: data.keybind,
			text: data.name,
			action: Action::OpenProject(data.project_data),
		});

	let projects = parse_ctx
		.get_projects()?
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
		banner: global_config.banner.clone(),
		colorscheme: global_config.colorscheme.clone(),
		sections: sections.collect(),
	};

	let action = tui::run(&tui_data).map_err(|err| err.to_string())?;
	match action {
		UserSelection::ControlC => Ok(ExitCode::SUCCESS),
		UserSelection::Button(action) => action.execute(global_config.global_project_data, parse_ctx),
	}
}

#[derive(Clone, Debug)]
enum Action {
	Run(Command),
	OpenProject(ProjectDataFuture),
}
impl Action {
	fn execute(
		self,
		parse_state: PrelimParseState,
		ctx: &mut ParseContext,
	) -> GenericResult<ExitCode> {
		match self {
			Action::Run(cmd) => cmd.run(),
			Action::OpenProject(project) => {
				let project_result = project.load(parse_state, ctx)?;
				project_result.open().map_err(|err| err.to_string().into())
			}
		}
	}
}

#[derive(Clone)]
pub struct CommandData {
	pub name: String,
	pub keybind: String,
	pub command: Command,
}
//TODO: make project's expressive enough to also handle this concept
#[derive(Clone, Debug)]
pub struct Command {
	pub command: Vec<String>,
	pub detach: bool,
}
impl Command {
	fn run(self) -> GenericResult<ExitCode> {
		if self.command.is_empty() {
			return Ok(ExitCode::SUCCESS);
		}
		let cmd = self.command[0].clone();
		let cmd_args = self.command.into_iter().skip(1);

		if self.detach {
			unistd::daemon(false, false).map_err(|err| format!("Failed to detach process: {err}"))?;
		}

		let mut child = OsCommand::new(&cmd)
			.args(cmd_args)
			.spawn()
			.map_err(|err| format!("Failed to execute command `{cmd}`: {err}"))?;

		let exit_status = child.wait().unwrap();

		if let Some(code) = exit_status.code() {
			Ok((code as u8).into())
		} else if exit_status.success() {
			Ok(ExitCode::SUCCESS)
		} else {
			Ok(ExitCode::FAILURE)
		}
	}
}
