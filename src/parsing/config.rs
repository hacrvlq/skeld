use std::path::Path;

use super::{
	ModResult, ParseContext,
	lib::{
		self as parse_lib, ArrayOption, BaseOption, BoolOption, ConfigOption, Diagnostic, StringOption,
		TomlKey, TomlValue, diagnostics,
	},
	project_data::{self, ProjectDataOption},
	string_interpolation,
};
use crate::{
	GlobalConfig,
	ui_subcommand::{Command, CommandData, tui},
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
	normal: tui::Color::Reset,
	banner: tui::Color::Reset,
	heading: tui::Color::Reset,
	keybind: tui::Color::Reset,
	button_label: tui::Color::Reset,
	background: tui::Color::Reset,
};
pub fn default_config() -> GlobalConfig {
	GlobalConfig {
		banner: DEFAULT_BANNER.to_string(),
		colorscheme: DEFAULT_COLORSCHEME,
		disable_help_text: false,
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
	let mut disable_help_text = BoolOption::new("disable-help");
	parse_lib::parse_table!(
		&parsed_contents => [
			global_project_data,
			commands,
			colorscheme,
			banner,
			disable_help_text
		],
		docs-section: "CONFIGURATION",
	)?;

	Ok(GlobalConfig {
		commands: commands.get_value().unwrap_or_default(),
		global_project_data: global_project_data.get_value(),
		colorscheme: colorscheme.get_value().unwrap_or(DEFAULT_COLORSCHEME),
		banner: banner.get_value().unwrap_or(DEFAULT_BANNER.to_string()),
		disable_help_text: disable_help_text.get_value().unwrap_or_default(),
	})
}
fn parse_command_data(value: &TomlValue) -> ModResult<CommandData> {
	let table = value.as_table()?;

	let mut name = StringOption::new("name");
	let mut keybind = StringOption::new("keybind");
	let mut command = ArrayOption::new("command", false, |raw_value| {
		let value = raw_value.as_str()?;
		string_interpolation::resolve_placeholders(value, false)
			.map_err(|err| diagnostics::failed_canonicalization(raw_value, &err).into())
	});
	let mut detach = BoolOption::new("detach");

	let docs_section = "CONFIGURATION";
	parse_lib::parse_table!(
		&table => [name, keybind, command, detach],
		docs-section: docs_section,
	)?;
	let name = name
		.get_value()
		.ok_or_else(|| diagnostics::missing_option(value.loc(), "name", docs_section))?;
	let keybind = keybind
		.get_value()
		.ok_or_else(|| diagnostics::missing_option(value.loc(), "keybind", docs_section))?;
	let command = command
		.get_value()
		.ok_or_else(|| diagnostics::missing_option(value.loc(), "command", docs_section))?;
	// detach' is useless if 'command' is empty,
	// as skeld will quit immediately in this case
	let detach = if command.is_empty() {
		detach.get_value().unwrap_or(false)
	} else {
		detach
			.get_value()
			.ok_or_else(|| diagnostics::missing_option(value.loc(), "detach", docs_section))?
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

	let create_color_option = |name| BaseOption::<tui::Color>::new(name, parse_tui_color);
	let mut normal = create_color_option("normal");
	let mut banner = create_color_option("banner");
	let mut heading = create_color_option("heading");
	let mut keybind = create_color_option("keybind");
	let mut button_label = create_color_option("label");
	let mut background = create_color_option("background");
	parse_lib::parse_table!(
		table => [normal, banner, heading, keybind, button_label, background],
		docs-section: "CONFIGURATION",
	)?;

	let mut resulting_colorscheme = DEFAULT_COLORSCHEME;
	macro_rules! handle_color_option {
		($opt_name:ident) => {
			if let Some(color) = $opt_name.get_value() {
				resulting_colorscheme.$opt_name = color;
			}
		};
	}
	handle_color_option!(normal);
	handle_color_option!(banner);
	handle_color_option!(heading);
	handle_color_option!(keybind);
	handle_color_option!(button_label);
	handle_color_option!(background);
	Ok(resulting_colorscheme)
}
fn parse_tui_color(value: &TomlValue) -> ModResult<tui::Color> {
	if let Ok(str) = value.as_str() {
		parse_hex_color(str).ok_or_else(|| {
			let label = value
				.loc()
				.get_primary_label()
				.with_message("expected format is #RRGGBB");
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
				.with_notes(vec![
					"(see https://en.wikipedia.org/wiki/ANSI_escape_code#8-bit)".to_string(),
				])
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
	if str.len() != 7 || !str.starts_with('#') || str.chars().nth(1).unwrap() == '+' {
		return None;
	}
	let str = &str[1..];

	let num = u64::from_str_radix(str, 16).ok()?;
	let r = (num >> 16) as u8;
	let g = (num >> 8) as u8;
	let b = num as u8;

	Some(tui::Color::Rgb { r, g, b })
}
