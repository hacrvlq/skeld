mod config;
mod lib;
mod path_util;
mod project_data;

use std::{
	fs, io,
	path::{Path, PathBuf},
};

use codespan_reporting::term::{self, termcolor};
use crossterm::tty::IsTty as _;

use lib::{self as parse_lib, diagnostics, FileDatabase, StringOption, TomlKey, TomlValue};

pub use project_data::{PrelimParseState, ProjectDataFuture};

#[derive(Clone, Debug, derive_more::From)]
pub enum Error {
	Diagnostic(lib::Diagnostic),
	Generic(String),
}
pub type Result<T> = core::result::Result<T, Error>;

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
pub struct ParseContext {
	file_database: FileDatabase,
}
impl ParseContext {
	pub fn new() -> Self {
		Self {
			file_database: FileDatabase::new(),
		}
	}
	pub fn print_error(&self, err: &Error) {
		match err {
			Error::Diagnostic(diag) => {
				let color_choice = if io::stderr().is_tty() {
					termcolor::ColorChoice::Auto
				} else {
					termcolor::ColorChoice::Never
				};
				let writer = termcolor::StandardStream::stderr(color_choice);
				let config = term::Config::default();
				let writer_error = term::emit(&mut writer.lock(), &config, &self.file_database, diag);
				if writer_error.is_err() {
					eprintln!(
						"Failed to pretty-print error, here is the raw version:\nerror: {}",
						diag.message
					);
				}
			}
			Error::Generic(msg) => eprintln!("{msg}"),
		}
	}

	pub fn get_global_config(&mut self) -> Result<config::GlobalConfig> {
		let global_config_file_path = path_util::canonicalize_path("$(CONFIG)/skeld/config.toml")
			.map_err(|err| format!("{err}"))?;
		if !global_config_file_path.exists() {
			return Ok(Default::default());
		}
		config::parse_config_file(&global_config_file_path, self)
	}
	pub fn get_projects(&mut self) -> Result<Vec<ProjectButtonData>> {
		let mut projects = Vec::new();
		for data_root_dir in path_util::get_data_root_paths().map_err(|err| format!("{err}"))? {
			let projects_root_dir = data_root_dir.join("projects");
			projects.append(&mut self.read_projects_from_dir(projects_root_dir)?);
		}
		let projects = sort_vec_and_check_dup(projects, |v| v.name.clone())
			.ok_or_else(|| "Found multiple projects with the same name".to_string())?;
		Ok(projects)
	}
	fn read_projects_from_dir(
		&mut self,
		projects_dir: impl AsRef<Path>,
	) -> Result<Vec<ProjectButtonData>> {
		let mut projects = Vec::new();
		for entry in get_toml_files_from_dir(projects_dir)? {
			let project_data = ProjectDataFuture::Project(entry.clone());
			let file_stem = entry.file_stem().unwrap();
			let project_name = file_stem
				.to_str()
				.ok_or_else(|| format!("file stem of `{}` is invalid UTF-8", entry.display()))?
				.to_string();

			projects.push(ProjectButtonData {
				project_data,
				name: project_name,
			});
		}
		Ok(projects)
	}
	pub fn get_bookmarks(&mut self) -> Result<Vec<BookmarkData>> {
		let mut bookmarks = Vec::new();
		for data_root_dir in path_util::get_data_root_paths().map_err(|err| format!("{err}"))? {
			let bookmarks_dir = data_root_dir.join("bookmarks/");
			bookmarks.append(&mut self.read_bookmarks_from_dir(bookmarks_dir)?);
		}
		let bookmarks = sort_vec_and_check_dup(bookmarks, |v| v.keybind.clone())
			.ok_or_else(|| "Found multiple bookmarks with the same key binding".to_string())?;
		Ok(bookmarks)
	}
	fn read_bookmarks_from_dir(
		&mut self,
		bookmarks_dir: impl AsRef<Path>,
	) -> Result<Vec<BookmarkData>> {
		let mut bookmarks = Vec::new();
		for entry in get_toml_files_from_dir(bookmarks_dir)? {
			bookmarks.push(self.parse_bookmark_file_stage1(entry)?);
		}
		Ok(bookmarks)
	}
	pub fn parse_bookmark_file_stage1(&mut self, path: impl AsRef<Path>) -> Result<BookmarkData> {
		let mut outlivers = (None, None);
		let parsed_contents =
			parse_lib::parse_toml_file(path.as_ref(), &mut self.file_database, &mut outlivers)?;

		let mut name = StringOption::new("name");
		let mut keybind = StringOption::new("keybind");
		// mock the project data option, so there is not an "unknown option" error
		struct ProjectDataMockOption;
		impl parse_lib::ConfigOption for ProjectDataMockOption {
			fn try_eat(&mut self, key: &TomlKey, _: &TomlValue) -> Result<bool> {
				Ok(key.name() == "project")
			}
		}
		let mut project_data = ProjectDataMockOption;
		parse_lib::parse_table!(&parsed_contents => [name, keybind, project_data])?;

		Ok(BookmarkData {
			name: name
				.get_value()
				.ok_or_else(|| diagnostics::missing_option(parsed_contents.loc(), "name"))?,
			keybind: keybind
				.get_value()
				.ok_or_else(|| diagnostics::missing_option(parsed_contents.loc(), "keybind"))?,
			project_data: ProjectDataFuture::Bookmark(path.as_ref().to_path_buf()),
		})
	}
}

fn get_toml_files_from_dir(dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
	let display_dir = dir.as_ref().display().to_string();
	let traverse_error_msg = |err| format!("Failed to traverse `{display_dir}: {err}`",).into();

	let dir_iter = match fs::read_dir(dir) {
		Ok(iter) => iter,
		Err(err) => match err.kind() {
			io::ErrorKind::NotFound => return Ok(Vec::new()),
			_ => return Err(traverse_error_msg(err)),
		},
	};

	let mut entries = Vec::new();
	for entry in dir_iter {
		let entry_path = entry.map_err(traverse_error_msg)?.path();
		if !entry_path.is_file() || !entry_path.extension().is_some_and(|ext| ext == "toml") {
			continue;
		}
		entries.push(entry_path);
	}

	Ok(entries)
}
fn sort_vec_and_check_dup<T, K: Eq + Ord>(
	mut vec: Vec<T>,
	key_fn: impl Fn(&T) -> K,
) -> Option<Vec<T>> {
	vec.sort_by_key(&key_fn);
	if vec
		.windows(2)
		.any(|window| key_fn(&window[0]) == key_fn(&window[1]))
	{
		return None;
	}
	Some(vec)
}
