use std::path::Path;

use super::{
	ModResult, ParseContext,
	lib::{
		self as parse_lib, ArrayOption, BaseOption, BoolOption, Diagnostic, IntegerOption,
		StringOption, TomlTable, TomlValue, diagnostics,
	},
	project_data::{self, ProjectDataOption},
	string_interpolation,
};
use crate::{GlobalConfig, command::Command, ui_subcommand::tui};

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
	project_name: tui::Color::Reset,
	background: tui::Color::Reset,
};
pub fn default_config() -> GlobalConfig {
	use tui::{KeyAction, KeyCode, KeyModifiers, Keybind};
	GlobalConfig {
		banner: DEFAULT_BANNER.to_string(),
		project_button_width: 40,
		colorscheme: DEFAULT_COLORSCHEME,
		keybinds: vec![
			Keybind {
				keys: vec![(KeyCode::Char('c'), KeyModifiers::CONTROL)].into(),
				action: KeyAction::Quit,
			},
			Keybind {
				keys: vec![(KeyCode::Enter, KeyModifiers::NONE)].into(),
				action: KeyAction::Choose,
			},
			Keybind {
				keys: vec![(KeyCode::Char('j'), KeyModifiers::NONE)].into(),
				action: KeyAction::MoveDown,
			},
			Keybind {
				keys: vec![(KeyCode::Down, KeyModifiers::NONE)].into(),
				action: KeyAction::MoveDown,
			},
			Keybind {
				keys: vec![(KeyCode::Char('k'), KeyModifiers::NONE)].into(),
				action: KeyAction::MoveUp,
			},
			Keybind {
				keys: vec![(KeyCode::Up, KeyModifiers::NONE)].into(),
				action: KeyAction::MoveUp,
			},
		],
		global_project_data: project_data::RawProjectData::empty(),
	}
}

pub fn parse_config_file(
	path: impl AsRef<Path>,
	ctx: &mut ParseContext,
) -> ModResult<GlobalConfig> {
	let mut outlivers = None;
	let parsed_contents =
		parse_lib::parse_toml_file(path.as_ref(), ctx.file_database, &mut outlivers)?;

	let mut global_project_data =
		ProjectDataOption::new("project", project_data::RawProjectData::empty(), ctx);
	let mut colorscheme = ColorschemeOption::new();
	let mut banner = StringOption::new("banner");
	let mut project_button_width = IntegerOption::new("project-button-width", 0..=u16::MAX as i64);
	let mut keybinds = ArrayOption::new("keybinds", true, parse_keybind);
	let mut disable_default_keybinds = BoolOption::new("disable-default-keybinds");
	parse_lib::parse_table!(
		parsed_contents => [
			global_project_data,
			colorscheme,
			banner,
			project_button_width,
			keybinds,
			disable_default_keybinds,
		],
		docs-section: "CONFIGURATION",
	)?;

	let mut config = default_config();
	config.global_project_data = global_project_data.get_value();
	if let Some(colorscheme) = colorscheme.get_value()? {
		config.colorscheme = colorscheme;
	}
	if let Some(banner) = banner.get_value()? {
		config.banner = banner;
	}
	if let Some(button_width) = project_button_width.get_value()? {
		config.project_button_width = button_width.try_into().unwrap();
	}
	if disable_default_keybinds.get_value()? == Some(true) {
		config.keybinds.clear();
	}
	if let Some(keybinds) = keybinds.get_value() {
		config.keybinds.extend(keybinds);
	}

	Ok(config)
}

fn parse_keybind(value: TomlValue) -> ModResult<tui::Keybind> {
	let docs_section = "CONFIGURATION";

	let mut table = value.into_table()?;

	let unparsed_keys = table
		.remove_entry("keys")
		.ok_or_else(|| diagnostics::missing_option(table.loc(), "keys", docs_section))?;
	let keys = parse_key_sequence(unparsed_keys.1)?;

	let unparsed_action = table
		.remove_entry("action")
		.ok_or_else(|| diagnostics::missing_option(table.loc(), "action", docs_section))?;
	let action = parse_key_action(unparsed_action.1)?;

	if let Some(unknown_entry) = table.into_iter().next() {
		return Err(diagnostics::unknown_option(&unknown_entry.0, docs_section).into());
	}

	Ok(tui::Keybind { keys, action })
}
fn parse_key_sequence(value: TomlValue) -> ModResult<tui::KeySequence> {
	let str = value.as_str()?;
	let mut keys = Vec::new();

	let mut unprocssed_str = str;
	while let Some(opening_bracket) = unprocssed_str.find('<') {
		keys.extend(
			unprocssed_str[..opening_bracket]
				.chars()
				.map(|ch| (tui::KeyCode::Char(ch), tui::KeyModifiers::NONE)),
		);
		unprocssed_str = &unprocssed_str[opening_bracket..];

		let Some(closing_bracket) = unprocssed_str.find('>') else {
			break;
		};
		let inner_str = &unprocssed_str[1..closing_bracket];

		if let Some(key) = parse_key_combination(inner_str) {
			keys.push(key);
			unprocssed_str = &unprocssed_str[closing_bracket + 1..];
		} else {
			// interpret the '<' as a char if it is not part of a valid key combination
			keys.push((tui::KeyCode::Char('<'), tui::KeyModifiers::NONE));
			unprocssed_str = &unprocssed_str[1..];
		}
	}

	keys.extend(
		unprocssed_str
			.chars()
			.map(|ch| (tui::KeyCode::Char(ch), tui::KeyModifiers::NONE)),
	);

	Ok(tui::KeySequence(keys))
}
fn parse_key_combination(mut str: &str) -> Option<(tui::KeyCode, tui::KeyModifiers)> {
	let mut modifiers = tui::KeyModifiers::NONE;
	let modifier_prefixes = [
		("s-", tui::KeyModifiers::SHIFT),
		("c-", tui::KeyModifiers::CONTROL),
		("a-", tui::KeyModifiers::ALT),
		("m-", tui::KeyModifiers::META),
		("d-", tui::KeyModifiers::SUPER),
	];
	loop {
		let mut detected_modifier = false;

		for (prefix, modifier) in modifier_prefixes {
			if let Some(trimmed_str) = str.strip_prefix(prefix) {
				modifiers.insert(modifier);
				str = trimmed_str;
				detected_modifier = true;
			}
		}

		if !detected_modifier {
			break;
		}
	}

	use tui::KeyCode;
	let keycode = match str.to_ascii_lowercase().as_str() {
		"nul" => KeyCode::Null,
		"bs" => KeyCode::Backspace,
		"tab" => KeyCode::Tab,
		"cr" => KeyCode::Enter,
		"return" => KeyCode::Enter,
		"enter" => KeyCode::Enter,
		"esc" => KeyCode::Esc,
		"space" => KeyCode::Char(' '),
		"lt" => KeyCode::Char('<'),
		"gt" => KeyCode::Char('>'),
		"del" => KeyCode::Delete,
		"up" => KeyCode::Up,
		"down" => KeyCode::Down,
		"left" => KeyCode::Left,
		"right" => KeyCode::Right,
		"f1" => KeyCode::F(1),
		"f2" => KeyCode::F(2),
		"f3" => KeyCode::F(3),
		"f4" => KeyCode::F(4),
		"f5" => KeyCode::F(5),
		"f6" => KeyCode::F(6),
		"f7" => KeyCode::F(7),
		"f8" => KeyCode::F(8),
		"f9" => KeyCode::F(9),
		"f10" => KeyCode::F(10),
		"f11" => KeyCode::F(11),
		"f12" => KeyCode::F(12),
		"insert" => KeyCode::Insert,
		"home" => KeyCode::Home,
		"end" => KeyCode::End,
		"pageup" => KeyCode::PageUp,
		"pagedown" => KeyCode::PageDown,
		_ => {
			let ch = assure_single_element(str.chars())?;
			KeyCode::Char(ch)
		}
	};

	Some((keycode, modifiers))
}
fn parse_key_action(value: TomlValue) -> ModResult<tui::KeyAction> {
	if let Ok(str) = value.as_str() {
		match str {
			"move_down" => Ok(tui::KeyAction::MoveDown),
			"move_up" => Ok(tui::KeyAction::MoveUp),
			"choose" => Ok(tui::KeyAction::Choose),
			"quit" => Ok(tui::KeyAction::Quit),
			"nop" => Ok(tui::KeyAction::Nop),
			_ => Err(
				Diagnostic::new(parse_lib::Severity::Error)
					.with_message("unknown action")
					.with_label(value.loc().get_primary_label())
					.into(),
			),
		}
	} else if value.is_table() {
		let table = value.into_table().unwrap();
		let command = parse_action_command(table)?;
		Ok(tui::KeyAction::LaunchProgram(command))
	} else {
		Err(
			diagnostics::wrong_type(
				&value,
				&[
					parse_lib::TomlInnerValue::String(Default::default()),
					parse_lib::TomlInnerValue::Array(Default::default()),
				],
			)
			.into(),
		)
	}
}
fn parse_action_command(table: TomlTable) -> ModResult<Command> {
	let table_loc = table.loc().clone();

	let mut cmd = ArrayOption::new("cmd", false, |raw_value| {
		let value = raw_value.as_str()?;
		Ok((value.to_owned(), raw_value.loc().clone()))
	});
	let mut detach = BoolOption::new("detach");

	let docs_section = "CONFIGURATION";
	parse_lib::parse_table!(
		table => [cmd, detach],
		docs-section: docs_section,
	)?;
	let cmd = cmd
		.get_value_with_loc()
		.ok_or_else(|| diagnostics::missing_option(&table_loc, "cmd", docs_section))?;
	let detach = detach
		.get_value()?
		.ok_or_else(|| diagnostics::missing_option(&table_loc, "detach", docs_section))?;

	let mut cmd_iter = cmd.0.into_iter();
	let program = cmd_iter.next().ok_or_else(|| {
		let label = cmd
			.1
			.get_primary_label()
			.with_message("command must not be empty");
		Diagnostic::new(parse_lib::Severity::Error)
			.with_message("empty command")
			.with_labels(vec![label])
	})?;
	let program = string_interpolation::resolve_placeholders(&program.0)
		.map_err(|err| diagnostics::failed_canonicalization(&program.1, &err))?;
	let args = cmd_iter.map(|(arg, _)| arg).collect();

	Ok(Command {
		program,
		args,
		detach,
		working_dir: None,
	})
}

parse_lib::wrap_BaseOption!(ColorschemeOption : tui::Colorscheme);
impl ColorschemeOption<'_> {
	fn new() -> Self {
		Self(BaseOption::new("colorscheme", parse_colorscheme))
	}
	fn get_value(self) -> ModResult<Option<tui::Colorscheme>> {
		self.0.get_value()
	}
}
fn parse_colorscheme(value: TomlValue) -> ModResult<tui::Colorscheme> {
	let table = value.into_table()?;

	let create_color_option = |name| BaseOption::<tui::Color>::new(name, parse_tui_color);
	let mut normal = create_color_option("normal");
	let mut banner = create_color_option("banner");
	let mut heading = create_color_option("heading");
	let mut keybind = create_color_option("keybind");
	let mut project_name = create_color_option("project-name");
	let mut background = create_color_option("background");
	parse_lib::parse_table!(
		table => [normal, banner, heading, keybind, project_name, background],
		docs-section: "CONFIGURATION",
	)?;

	let mut resulting_colorscheme = DEFAULT_COLORSCHEME;
	macro_rules! handle_color_option {
		($opt_name:ident) => {
			if let Some(color) = $opt_name.get_value()? {
				resulting_colorscheme.$opt_name = color;
			}
		};
	}
	handle_color_option!(normal);
	handle_color_option!(banner);
	handle_color_option!(heading);
	handle_color_option!(keybind);
	handle_color_option!(project_name);
	handle_color_option!(background);
	Ok(resulting_colorscheme)
}
fn parse_tui_color(value: TomlValue) -> ModResult<tui::Color> {
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
				&value,
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

fn assure_single_element<T>(mut iter: impl Iterator<Item = T>) -> Option<T> {
	if let Some(first_element) = iter.next()
		&& iter.next().is_none()
	{
		Some(first_element)
	} else {
		None
	}
}
