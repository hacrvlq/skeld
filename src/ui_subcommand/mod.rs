pub mod tui;

use std::{
	collections::HashSet,
	process::{Command as OsCommand, ExitCode},
};

use self::tui::{TuiData, UserSelection};
use crate::{
	GenericResult,
	parsing::{ParseContext, PrelimParseState, ProjectDataFuture},
};

pub fn run(
	parse_ctx: &mut ParseContext,
	global_config: crate::GlobalConfig,
) -> GenericResult<ExitCode> {
	let commands = global_config.commands;
	let bookmarks = parse_ctx.get_bookmarks()?;
	let projects = parse_ctx.get_projects()?;

	// stores all keybinds that are positive numbers
	// This is used to find the first available numeric keybind for projects that
	// don't provide a keybinding themselves.
	let mut numeric_keybinds = bookmarks
		.iter()
		.chain(projects.iter())
		.filter_map(|data| data.keybind.clone())
		.chain(commands.iter().map(|data| data.keybind.clone()))
		.filter_map(parse_str_as_num)
		.collect::<HashSet<_>>();

	let commands = commands.into_iter().map(|data| tui::Button {
		keybind: data.keybind,
		text: data.name,
		action: Action::Run(data.command),
	});

	let projects_sections = [
		("Bookmarks", parse_ctx.get_bookmarks()?),
		("Projects", parse_ctx.get_projects()?),
	]
	.map(|(heading, projects)| tui::Section {
		heading: heading.to_string(),
		buttons: projects
			.into_iter()
			.map(|project| tui::Button {
				keybind: project.keybind.unwrap_or_else(|| {
					let first_unused_num = (1..).find(|i| numeric_keybinds.insert(*i)).unwrap();
					first_unused_num.to_string()
				}),
				text: project.name,
				action: Action::OpenProject(project.project_data),
			})
			.collect::<Vec<_>>(),
	});

	let sections = [
		vec![tui::Section {
			heading: "Commands".to_string(),
			buttons: commands.collect(),
		}],
		projects_sections.to_vec(),
	]
	.concat()
	.into_iter()
	.filter(|section| !section.buttons.is_empty())
	.map(|section| {
		let (mut buttons_numerical, buttons_rest): (Vec<_>, Vec<_>) = section
			.buttons
			.into_iter()
			.partition(|button| parse_str_as_num(&button.keybind).is_some());
		buttons_numerical.sort_by_key(|button| parse_str_as_num(&button.keybind).unwrap());
		let buttons = [buttons_rest, buttons_numerical].concat();

		tui::Section { buttons, ..section }
	})
	.collect::<Vec<_>>();

	let help_text = if global_config.disable_help_text {
		"".to_string()
	} else {
		"Use J/K/Enter/Mouse to navigate".to_string()
	};

	let tui_data = TuiData {
		banner: global_config.banner.clone(),
		colorscheme: global_config.colorscheme.clone(),
		sections,
		help_text,
	};

	let action = tui::run(&tui_data).map_err(|err| err.to_string())?;
	match action {
		UserSelection::ControlC => Ok(ExitCode::SUCCESS),
		UserSelection::Button(action) => action.execute(global_config.global_project_data, parse_ctx),
	}
}

// parse str as a positive number, disallowing a leading '+'
fn parse_str_as_num(str: impl AsRef<str>) -> Option<u64> {
	let str = str.as_ref();
	if str.starts_with('+') {
		None
	} else {
		str.parse::<u64>().ok()
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
			crate::command::detach_from_tty()?;
		}

		let mut child = OsCommand::new(&cmd)
			.args(cmd_args)
			.spawn()
			.map_err(|err| format!("Failed to execute command `{cmd}`: {err}"))?;

		let exit_status = child.wait().unwrap();
		Ok(crate::command::forward_child_exit_status(exit_status))
	}
}
