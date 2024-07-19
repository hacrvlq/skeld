use std::path::Path;

use crate::{action::Command, tui};

use super::{
	lib::{self as parse_lib, diagnostics, ArrayOption, BoolOption, StringOption, TomlValue},
	path_util,
	project_data::{self, ProjectDataOption},
	ParseContext, Result,
};

//TODO: make banner and colorscheme configurable
pub struct GlobalConfig {
	pub banner: String,
	pub colorscheme: tui::Colorscheme,
	pub commands: Vec<CommandData>,
	pub global_config_data: project_data::PrelimParseState,
}
#[derive(Clone)]
pub struct CommandData {
	pub name: String,
	pub keybind: String,
	pub command: Command,
}
impl Default for GlobalConfig {
	fn default() -> Self {
		GlobalConfig {
			banner: DEFAULT_BANNER.to_string(),
			colorscheme: DEFAULT_COLORSCHEME,
			commands: Vec::new(),
			global_config_data: project_data::PrelimParseState::empty(),
		}
	}
}
// generated with FIGlet using the larry3d font
const DEFAULT_BANNER: &str = r#"
       __              ___       __
      /\ \            /\_ \     /\ \
  ____\ \ \/'\      __\//\ \    \_\ \
 /  __\\ \   <    / __ \\ \ \   / _  \
/\__, `\\ \ \\'\ /\  __/ \_\ \_/\ \_\ \
\/\____/ \ \_\ \_\ \____\/\____\ \_____\
 \/___/   \/_/\/_/\/____/\/____/\/____ /
"#;
const DEFAULT_COLORSCHEME: tui::Colorscheme = tui::Colorscheme {
	neutral: tui::Color::Reset,
	banner: tui::Color::Yellow,
	heading: tui::Color::DarkYellow,
	keybind: tui::Color::DarkCyan,
	button_label: tui::Color::DarkGrey,
};

pub fn parse_config_file(path: impl AsRef<Path>, ctx: &mut ParseContext) -> Result<GlobalConfig> {
	let mut outlivers = (None, None);
	let parsed_contents =
		parse_lib::parse_toml_file(path.as_ref(), &mut ctx.file_database, &mut outlivers)?;

	let mut global_project_data =
		ProjectDataOption::new("project", project_data::PrelimParseState::empty(), ctx);
	let mut commands = ArrayOption::new("commands", false, parse_command_data);
	parse_lib::parse_table!(&parsed_contents => [global_project_data, commands])?;

	Ok(GlobalConfig {
		commands: commands.get_value().unwrap_or_default(),
		global_config_data: global_project_data.get_value(),
		..Default::default()
	})
}
fn parse_command_data(value: &TomlValue) -> Result<CommandData> {
	let table = value.as_table()?;

	let mut name = StringOption::new("name");
	let mut keybind = StringOption::new("keybind");
	let mut command = ArrayOption::new("command", false, |raw_value| {
		let value = raw_value.as_str()?;
		path_util::substitute_placeholder(value, false)
			.map_err(|err| diagnostics::failed_canonicalization(raw_value, &err).into())
	});
	let mut detach = BoolOption::new("detach");

	parse_lib::parse_table!(&table => [name, keybind, command, detach])?;

	Ok(CommandData {
		name: name
			.get_value()
			.ok_or_else(|| diagnostics::missing_option(value.loc(), "name"))?,
		keybind: keybind
			.get_value()
			.ok_or_else(|| diagnostics::missing_option(value.loc(), "name"))?,
		command: Command {
			command: command
				.get_value()
				.ok_or_else(|| diagnostics::missing_option(value.loc(), "command"))?,
			detach: detach
				.get_value()
				.ok_or_else(|| diagnostics::missing_option(value.loc(), "detach"))?,
		},
	})
}
