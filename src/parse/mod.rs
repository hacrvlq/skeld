mod config;
mod lib;
mod path;
mod project_data;

use std::{
	fs, io,
	path::{Path, PathBuf},
};

use self::lib::{self as parse_lib, diagnostics, StringOption, TomlKey, TomlValue};
use crate::{dirs, GlobalConfig};

pub use self::{
	lib::{Diagnostic, FileDatabase},
	project_data::{PrelimParseState, ProjectDataFuture},
};

type ModResult<T> = crate::GenericResult<T>;

#[derive(Clone)]
pub struct ProjectButtonData {
	pub name: String,
	pub project_data: ProjectDataFuture,
}
#[derive(Clone)]
pub struct BookmarkData {
	pub project_data: ProjectDataFuture,
	pub keybind: String,
	pub name: String,
}

// NOTE: FileDatabase is required for displaying errors,
//       therefore it is stored globally
pub struct ParseContext<'a> {
	pub file_database: &'a mut FileDatabase,
}
impl ParseContext<'_> {
	pub fn get_global_config(
		&mut self,
		config_file_path: Option<impl AsRef<Path>>,
	) -> ModResult<GlobalConfig> {
		let global_config_file_path = match config_file_path {
			Some(path) => path.as_ref().to_path_buf(),
			None => dirs::get_skeld_config_dir()
				.map_err(|err| format!("Failed to determine the skeld config dir:\n  {err}"))?
				.join("config.toml"),
		};

		if !global_config_file_path.exists() {
			return Ok(config::default_config());
		}

		config::parse_config_file(&global_config_file_path, self)
	}
	pub fn get_projects(&mut self) -> ModResult<Vec<ProjectButtonData>> {
		let mut projects = Vec::new();

		let skeld_data_dirs = dirs::get_skeld_data_dirs()
			.map_err(|err| format!("Failed to determine the skeld data directories:\n  {err}"))?;
		for data_root_dir in skeld_data_dirs {
			let projects_root_dir = data_root_dir.join("projects");
			projects.append(&mut self.read_projects_from_dir(projects_root_dir)?);
		}

		let projects =
			sort_vec_and_check_dup(projects, |v| v.1.name.clone()).map_err(|duplicates| {
				let duplicates_str = duplicates
					.iter()
					.map(|(path, _)| format!("- {}", path.display()))
					.collect::<Vec<_>>()
					.join("\n");
				format!(
					"Found conflicting projects with the same name `{}`:\n{duplicates_str}",
					duplicates[0].1.name
				)
			})?;

		let projects = projects.into_iter().map(|(_, data)| data).collect();
		Ok(projects)
	}
	fn read_projects_from_dir(
		&mut self,
		projects_dir: impl AsRef<Path>,
	) -> ModResult<Vec<(PathBuf, ProjectButtonData)>> {
		let mut projects = Vec::new();
		for entry in get_toml_files_from_dir(projects_dir)? {
			let project_data = ProjectDataFuture::Project(entry.clone());
			let file_stem = entry.file_stem().unwrap();
			let project_name = file_stem
				.to_str()
				.ok_or_else(|| {
					format!(
						concat!(
							"Failed to determine project name of `{}`,\n",
							"because file stem contains invalid UTF-8"
						),
						entry.display()
					)
				})?
				.to_string();
			let project_button_data = ProjectButtonData {
				project_data,
				name: project_name,
			};

			projects.push((entry, project_button_data));
		}
		Ok(projects)
	}
	pub fn get_bookmarks(&mut self) -> ModResult<Vec<BookmarkData>> {
		let mut bookmarks = Vec::new();

		let skeld_data_dirs = dirs::get_skeld_data_dirs()
			.map_err(|err| format!("Failed to determine the skeld data directories:\n  {err}"))?;
		for data_root_dir in skeld_data_dirs {
			let bookmarks_dir = data_root_dir.join("bookmarks/");
			bookmarks.append(&mut self.read_bookmarks_from_dir(bookmarks_dir)?);
		}

		let bookmarks =
			sort_vec_and_check_dup(bookmarks, |v| v.1.keybind.clone()).map_err(|duplicates| {
				let duplicates_str = duplicates
					.iter()
					.map(|(path, _)| format!("- {}", path.display()))
					.collect::<Vec<_>>()
					.join("\n");
				format!(
					"Found conflicting bookmarks with the same keybind `{}`:\n{duplicates_str}",
					duplicates[0].1.keybind
				)
			})?;

		let bookmarks = bookmarks.into_iter().map(|(_, data)| data).collect();
		Ok(bookmarks)
	}
	fn read_bookmarks_from_dir(
		&mut self,
		bookmarks_dir: impl AsRef<Path>,
	) -> ModResult<Vec<(PathBuf, BookmarkData)>> {
		let mut bookmarks = Vec::new();
		for entry in get_toml_files_from_dir(bookmarks_dir)? {
			let bookmark_data = self.parse_bookmark_file_stage1(&entry)?;
			bookmarks.push((entry, bookmark_data));
		}
		Ok(bookmarks)
	}
	pub fn parse_bookmark_file_stage1(&mut self, path: impl AsRef<Path>) -> ModResult<BookmarkData> {
		let mut outlivers = (None, None);
		let parsed_contents =
			parse_lib::parse_toml_file(path.as_ref(), self.file_database, &mut outlivers)?;

		let mut name = StringOption::new("name");
		let mut keybind = StringOption::new("keybind");
		// mock the project data option, so there is not an "unknown option" error
		struct ProjectDataMockOption;
		impl parse_lib::ConfigOption for ProjectDataMockOption {
			fn try_eat(&mut self, key: &TomlKey, _: &TomlValue) -> ModResult<bool> {
				Ok(key.name() == "project")
			}
		}
		let mut project_data = ProjectDataMockOption;

		let docs_pref = "bookmarks";
		parse_lib::parse_table!(
			&parsed_contents => [name, keybind, project_data],
			docs-pref: docs_pref,
		)?;
		Ok(BookmarkData {
			name: name
				.get_value()
				.ok_or_else(|| diagnostics::missing_option(parsed_contents.loc(), "name", docs_pref))?,
			keybind: keybind
				.get_value()
				.ok_or_else(|| diagnostics::missing_option(parsed_contents.loc(), "keybind", docs_pref))?,
			project_data: ProjectDataFuture::Bookmark(path.as_ref().to_path_buf()),
		})
	}
}

fn get_toml_files_from_dir(dir: impl AsRef<Path>) -> ModResult<Vec<PathBuf>> {
	let dir = dir.as_ref();

	let dir_iter = match fs::read_dir(dir) {
		Ok(iter) => iter,
		Err(err) => match err.kind() {
			io::ErrorKind::NotFound => return Ok(Vec::new()),
			_ => {
				return Err(format!("Failed to traverse directory `{}`:\n  {err}", dir.display()).into())
			}
		},
	};

	let mut entries = Vec::new();
	for entry in dir_iter {
		let entry_path = entry.unwrap().path();
		if !entry_path.is_file() || !entry_path.extension().is_some_and(|ext| ext == "toml") {
			continue;
		}
		entries.push(entry_path);
	}

	Ok(entries)
}
// if 'vec' has no duplicates, the sorted 'vec' is returned;
// otherwise a group of duplicates is returned as an error
fn sort_vec_and_check_dup<T, K: Eq + Ord>(
	mut vec: Vec<T>,
	key_fn: impl Fn(&T) -> K,
) -> Result<Vec<T>, Vec<T>> {
	vec.sort_by_key(&key_fn);

	let mut last_eq_idx = 0;
	// NOTE: This loop includes i = vec.len() as a "virtual" element
	//       that is different from all other element. This is
	//       necessary to detect duplicates at the end of 'vec'.
	for i in 1..=vec.len() {
		if i != vec.len() && key_fn(&vec[i]) == key_fn(&vec[last_eq_idx]) {
			continue;
		}

		if i - last_eq_idx > 1 {
			let duplicates = vec.drain(last_eq_idx..i).collect();
			return Err(duplicates);
		}

		last_eq_idx = i;
	}

	Ok(vec)
}
