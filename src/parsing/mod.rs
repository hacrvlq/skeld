mod config;
mod lib;
mod project_data;
mod string_interpolation;

use std::{
	fs, io,
	path::{Path, PathBuf},
};

use self::{
	lib::{self as parse_lib, MockOption, StringOption},
	project_data::{IntoProjectDataError, ProjectDataOption},
};
use crate::{
	GlobalConfig, dirs,
	project::{ProjectData, ProjectDataFile, ProjectFileData},
};

pub use self::{
	lib::{Diagnostic, FileDatabase},
	project_data::RawProjectData,
};

type ModResult<T> = crate::GenericResult<T>;

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
	pub fn get_bookmarks(&mut self) -> ModResult<Vec<ProjectFileData>> {
		self.get_projects_from_data_subdir("bookmarks")
	}
	pub fn get_projects(&mut self) -> ModResult<Vec<ProjectFileData>> {
		self.get_projects_from_data_subdir("projects")
	}
	pub fn parse_project_file(
		&mut self,
		path: impl AsRef<Path>,
		initial_data: RawProjectData,
	) -> ModResult<ProjectData> {
		let mut outlivers = (None, None);
		let parsed_contents =
			parse_lib::parse_toml_file(path.as_ref(), self.file_database, &mut outlivers)?;

		let mut name = MockOption::new("name");
		let mut keybind = MockOption::new("keybind");
		let mut project_data = ProjectDataOption::new("project", initial_data, self);

		let docs_section = "PROJECTS";
		parse_lib::parse_table!(
			&parsed_contents => [name, keybind, project_data],
			docs-section: docs_section,
		)?;
		let project_data = project_data
			.get_value()
			.into_project_data()
			.map_err(|err| match err {
				IntoProjectDataError::MissingConfigOption(missing) => {
					lib::diagnostics::missing_option(parsed_contents.loc(), &missing, docs_section).into()
				}
				IntoProjectDataError::Other(err) => err,
			})?;

		Ok(project_data)
	}

	// get projects from all '<SKELD-DATA>/subdir' directories
	fn get_projects_from_data_subdir(
		&mut self,
		subdir: impl AsRef<Path>,
	) -> ModResult<Vec<ProjectFileData>> {
		let mut projects = Vec::new();

		let skeld_data_dirs = dirs::get_skeld_data_dirs()
			.map_err(|err| format!("Failed to determine the skeld data directories:\n  {err}"))?;
		for data_root_dir in skeld_data_dirs {
			let projects_dir = data_root_dir.join(&subdir);
			let mut projects_in_dir = get_toml_files_from_dir(projects_dir)?
				.into_iter()
				.map(|entry| self.parse_project_file_stage1(entry))
				.collect::<ModResult<Vec<_>>>()?;
			projects.append(&mut projects_in_dir);
		}

		Ok(projects)
	}
	fn parse_project_file_stage1(&mut self, path: impl AsRef<Path>) -> ModResult<ProjectFileData> {
		let path = path.as_ref();

		let mut outlivers = (None, None);
		let parsed_contents = parse_lib::parse_toml_file(path, self.file_database, &mut outlivers)?;

		let mut name = StringOption::new("name");
		let mut keybind = StringOption::new("keybind");
		// mock the project data option, so there is not an "unknown option" error
		let mut project_data = MockOption::new("project");

		let docs_section = "PROJECTS";
		parse_lib::parse_table!(
			&parsed_contents => [name, keybind, project_data],
			docs-section: docs_section,
		)?;

		let project_name = match name.get_value() {
			Some(name) => name,
			None => {
				let file_stem = path.file_stem().unwrap();
				file_stem
					.to_str()
					.ok_or_else(|| {
						format!(
							concat!(
								"Failed to determine project name of `{}` from the filename,\n",
								"as it contains contains invalid UTF-8.\n",
								"  NOTE: use the config option 'name' to manually specify a name\n",
								"  (run `{man_cmd}` for more information)",
							),
							path.display(),
							man_cmd = crate::error::get_manpage_cmd(docs_section),
						)
					})?
					.to_string()
			}
		};

		Ok(ProjectFileData {
			name: project_name,
			keybind: keybind.get_value(),
			project_data_file: ProjectDataFile(path.to_path_buf()),
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
				return Err(format!("Failed to traverse directory `{}`:\n  {err}", dir.display()).into());
			}
		},
	};

	let mut entries = Vec::new();
	for entry in dir_iter {
		let entry_path = entry.unwrap().path();
		if !entry_path.is_file() || entry_path.extension().is_none_or(|ext| ext != "toml") {
			continue;
		}
		entries.push(entry_path);
	}

	// consistent order
	entries.sort();
	Ok(entries)
}
