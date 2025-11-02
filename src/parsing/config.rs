use std::path::Path;

use super::{
	ModResult, ParseContext,
	lib::{
		self as parse_lib, BaseOption, Diagnostic, IntegerOption, StringOption, TomlValue, diagnostics,
	},
	project_data::{self, ProjectDataOption},
};
use crate::{GlobalConfig, ui_subcommand::tui};

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
	GlobalConfig {
		banner: DEFAULT_BANNER.to_string(),
		project_button_width: 40,
		colorscheme: DEFAULT_COLORSCHEME,
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
	parse_lib::parse_table!(
		parsed_contents => [
			global_project_data,
			colorscheme,
			banner,
			project_button_width
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

	Ok(config)
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
