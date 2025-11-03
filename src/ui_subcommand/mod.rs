pub mod tui;

use std::{collections::HashSet, process::ExitCode};

use self::tui::{ProjectButton, ProjectsSection, TuiData, UiError};
use crate::{GenericResult, parsing::ParseContext};

pub fn run(
	parse_ctx: &mut ParseContext,
	global_config: crate::GlobalConfig,
) -> GenericResult<ExitCode> {
	let bookmarks = parse_ctx.get_bookmarks()?;
	let projects = parse_ctx.get_projects()?;

	// stores all keybinds that are positive numbers
	// This is used to find the first available numeric keybind for projects that
	// don't provide a keybinding themselves.
	let mut numeric_keybinds = Iterator::chain(bookmarks.iter(), projects.iter())
		.filter_map(|data| data.keybind.clone())
		.filter_map(parse_str_as_num)
		.collect::<HashSet<_>>();

	let projects_sections = [("Bookmarks", bookmarks), ("Projects", projects)]
		.into_iter()
		.filter(|section| !section.1.is_empty())
		.map(|(heading, projects)| {
			let buttons = projects.into_iter().map(|project| ProjectButton {
				keybind: project.keybind.unwrap_or_else(|| {
					let first_unused_num = (1..).find(|i| numeric_keybinds.insert(*i)).unwrap();
					first_unused_num.to_string()
				}),
				project_name: project.name,
				project: project.project_data_file,
			});

			let (mut buttons_numerical, mut buttons_rest): (Vec<_>, Vec<_>) =
				buttons.partition(|button| parse_str_as_num(&button.keybind).is_some());
			buttons_numerical.sort_by_key(|button| parse_str_as_num(&button.keybind).unwrap());
			buttons_rest
				.sort_by(|a, b| (a.keybind.len(), &a.keybind).cmp(&(b.keybind.len(), &a.keybind)));
			#[expect(clippy::tuple_array_conversions)]
			let buttons = [buttons_rest, buttons_numerical].concat();

			ProjectsSection {
				heading: heading.to_string(),
				buttons,
			}
		});

	let tui_data = TuiData {
		colorscheme: global_config.colorscheme,
		banner: global_config.banner.clone(),
		sections: projects_sections.collect(),
		project_button_width: global_config.project_button_width,
	};

	tui::run(&tui_data, global_config.global_project_data, parse_ctx).map_err(|err| match err {
		UiError::NoTty => "The skeld ui can only be used in a tty.".into(),
		UiError::IoError(err) => format!("An IO error occurred while rendering the tui: {err}").into(),
		UiError::Other(err) => err,
	})
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
