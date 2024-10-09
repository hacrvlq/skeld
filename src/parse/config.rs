use std::path::Path;

use super::{
	lib::{
		self as parse_lib, diagnostics, ArrayOption, BaseOption, BoolOption, ConfigOption, Diagnostic,
		StringOption, TomlKey, TomlValue,
	},
	path,
	project_data::{self, ProjectDataOption},
	ModResult, ParseContext,
};
use crate::{
	ui_subcommand::{tui, Command, CommandData},
	GlobalConfig,
};

// generated with FIGlet using the larry3d font
const DEFAULT_BANNER: &str = r"
       __              ___       __
      /\ \            /\_ \     /\ \
  ____\ \ \/'\      __\//\ \    \_\ \
 /  __\\ \   <    / __ \\ \ \   / _  \
/\__, `\\ \ \\'\ /\  __/ \_\ \_/\ \_\ \
\/\____/ \ \_\ \_\ \____\/\____\ \_____\
 \/___/   \/_/\/_/\/____/\/____/\/____ /
";
const DEFAULT_COLORSCHEME: tui::Colorscheme = tui::Colorscheme {
	neutral: tui::Color::Reset,
	banner: tui::Color::Yellow,
	heading: tui::Color::DarkYellow,
	keybind: tui::Color::DarkCyan,
	button_label: tui::Color::DarkGrey,
};
pub fn default_config() -> GlobalConfig {
	GlobalConfig {
		banner: DEFAULT_BANNER.to_string(),
		colorscheme: DEFAULT_COLORSCHEME,
		commands: Vec::new(),
		global_project_data: project_data::PrelimParseState::empty(),
	}
}

pub fn parse_config_file(
	path: impl AsRef<Path>,
	ctx: &mut ParseContext,
) -> ModResult<GlobalConfig> {
	let mut outlivers = (None, None);
	let parsed_contents =
		parse_lib::parse_toml_file(path.as_ref(), ctx.file_database, &mut outlivers)?;

	let mut global_project_data =
		ProjectDataOption::new("project", project_data::PrelimParseState::empty(), ctx);
	let mut commands = ArrayOption::new("commands", false, parse_command_data);
	let mut colorscheme = ColorschemeOption::new();
	let mut banner = StringOption::new("banner");
	parse_lib::parse_table!(&parsed_contents => [global_project_data, commands, colorscheme, banner])?;

	Ok(GlobalConfig {
		commands: commands.get_value().unwrap_or_default(),
		global_project_data: global_project_data.get_value(),
		colorscheme: colorscheme.get_value().unwrap_or(DEFAULT_COLORSCHEME),
		banner: banner.get_value().unwrap_or(DEFAULT_BANNER.to_string()),
	})
}
fn parse_command_data(value: &TomlValue) -> ModResult<CommandData> {
	let table = value.as_table()?;

	let mut name = StringOption::new("name");
	let mut keybind = StringOption::new("keybind");
	let mut command = ArrayOption::new("command", false, |raw_value| {
		let value = raw_value.as_str()?;
		path::substitute_placeholder(value, false)
			.map_err(|err| diagnostics::failed_canonicalization(raw_value, &err).into())
	});
	let mut detach = BoolOption::new("detach");

	parse_lib::parse_table!(&table => [name, keybind, command, detach])?;
	let name = name
		.get_value()
		.ok_or_else(|| diagnostics::missing_option(value.loc(), "name"))?;
	let keybind = keybind
		.get_value()
		.ok_or_else(|| diagnostics::missing_option(value.loc(), "name"))?;
	let command = command
		.get_value()
		.ok_or_else(|| diagnostics::missing_option(value.loc(), "command"))?;
	// detach' is useless if 'command' is empty,
	// as skeld will quit immediately in this case
	let detach = if command.is_empty() {
		detach.get_value().unwrap_or(false)
	} else {
		detach
			.get_value()
			.ok_or_else(|| diagnostics::missing_option(value.loc(), "detach"))?
	};

	Ok(CommandData {
		name,
		keybind,
		command: Command { command, detach },
	})
}

#[derive(Clone)]
struct ColorschemeOption(BaseOption<tui::Colorscheme>);
impl ColorschemeOption {
	fn new() -> Self {
		Self(BaseOption::new("colorscheme", parse_colorscheme))
	}
	fn get_value(self) -> Option<tui::Colorscheme> {
		self.0.get_value()
	}
}
impl ConfigOption for ColorschemeOption {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		self.0.try_eat(key, value)
	}
}
fn parse_colorscheme(value: &TomlValue) -> ModResult<tui::Colorscheme> {
	let table = value.as_table()?;

	let mut neutral = ColorOption::new("neutral");
	let mut banner = ColorOption::new("banner");
	let mut heading = ColorOption::new("heading");
	let mut keybind = ColorOption::new("keybind");
	let mut button_label = ColorOption::new("label");
	parse_lib::parse_table!(table => [neutral, banner, heading, keybind, button_label])?;

	let mut resulting_colorscheme = DEFAULT_COLORSCHEME;
	macro_rules! handle_color_option {
		($opt_name:ident) => {
			if let Some(color) = $opt_name.get_value() {
				resulting_colorscheme.$opt_name = color;
			}
		};
	}
	handle_color_option!(neutral);
	handle_color_option!(banner);
	handle_color_option!(heading);
	handle_color_option!(keybind);
	handle_color_option!(button_label);
	Ok(resulting_colorscheme)
}

#[derive(Clone)]
struct ColorOption(BaseOption<tui::Color>);
impl ColorOption {
	fn new(name: &str) -> Self {
		Self(BaseOption::new(name, parse_tui_color))
	}
	fn get_value(self) -> Option<tui::Color> {
		self.0.get_value()
	}
}
impl ConfigOption for ColorOption {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		self.0.try_eat(key, value)
	}
}
fn parse_tui_color(value: &TomlValue) -> ModResult<tui::Color> {
	if let Ok(str) = value.as_str() {
		parse_hex_color(str).ok_or_else(|| {
			let label = value
				.loc()
				.get_primary_label()
				.with_message("expected #RRGGBB");
			Diagnostic::new(parse_lib::Severity::Error)
				.with_message("invalid hex color")
				.with_labels(vec![label])
				.into()
		})
	} else if let Ok(num) = value.as_int() {
		let ansi_val: u8 = num.try_into().map_err(|_| {
			let label = value
				.loc()
				.get_primary_label()
				.with_message("must be in the range [0, 255]");
			Diagnostic::new(parse_lib::Severity::Error)
				.with_message("invalid ansi color")
				.with_labels(vec![label])
		})?;
		Ok(tui::Color::AnsiValue(ansi_val))
	} else {
		Err(
			diagnostics::wrong_type(
				value,
				&[
					parse_lib::TomlInnerValue::String(Default::default()),
					parse_lib::TomlInnerValue::Integer(Default::default()),
				],
			)
			.into(),
		)
	}
}
fn parse_hex_color(str: &str) -> Option<tui::Color> {
	if !str.starts_with('#') {
		return None;
	}
	let str = &str[1..];

	let num = i64::from_str_radix(str, 16).ok()?;
	if !(0..(1 << (4 * 8))).contains(&num) {
		return None;
	}

	let r = ((num >> 16) & 0xFF) as u8;
	let g = ((num >> 8) & 0xFF) as u8;
	let b = (num & 0xFF) as u8;

	Some(tui::Color::Rgb { r, g, b })
}
