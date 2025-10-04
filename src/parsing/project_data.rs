use std::path::{Path, PathBuf};

use super::{
	ModResult, ParseContext,
	lib::{
		self as parse_lib, ArrayOption, BoolOption, PathBufOption, StringOption, TomlKey, TomlTable,
		TomlValue, diagnostics,
	},
	string_interpolation,
};
use crate::{
	command::Command,
	parsing::lib::{CanonicalizationError, CanonicalizationLabel},
	project::ProjectData,
	sandbox::{EnvVarWhitelist, FSTreeError, SandboxParameters, VirtualFSEntryType, VirtualFSTree},
};

pub struct ProjectDataOption<'a, 'b> {
	name: String,
	value: RawProjectData,
	ctx: &'a mut ParseContext<'b>,
}
impl<'a, 'b> ProjectDataOption<'a, 'b> {
	pub fn new(name: &str, initial_data: RawProjectData, ctx: &'a mut ParseContext<'b>) -> Self {
		Self {
			name: name.to_string(),
			value: initial_data,
			ctx,
		}
	}
	pub fn get_value(self) -> RawProjectData {
		self.value
	}
}
impl parse_lib::ConfigOption for ProjectDataOption<'_, '_> {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		if key.name() != self.name {
			return Ok(false);
		}
		let table = value.as_table()?;
		self.value.parse_table(&table, self.ctx)?;
		Ok(true)
	}
}

// for each config option the configured value is saved or
// nothing if this config option has not yet been specified
#[derive(Clone)]
pub struct RawProjectData {
	project_dir: PathBufOption,
	initial_file: StringOption,
	editor: EditorCommandOption,
	virtual_fs: VirtualFSOption,
	whitelist_envvars: ArrayOption<String>,
	whitelist_all_envvars: BoolOption,
	disable_sandbox: BoolOption,

	parsed_files: Vec<PathBuf>,
}
impl RawProjectData {
	pub(super) fn empty() -> Self {
		Self {
			project_dir: PathBufOption::new("project-dir", canonicalize_path),
			initial_file: StringOption::new_with_canonicalization("initial-file", |str| {
				string_interpolation::resolve_placeholders(str, false)
			}),
			editor: EditorCommandOption::new(),
			virtual_fs: VirtualFSOption::new(),
			whitelist_envvars: ArrayOption::new("whitelist-envvar", true, |value| {
				let value = value.as_str()?;
				Ok(value.to_string())
			}),
			whitelist_all_envvars: BoolOption::new("whitelist-all-envvars"),
			disable_sandbox: BoolOption::new("no-sandbox"),

			parsed_files: Vec::new(),
		}
	}
	pub(super) fn into_project_data(self) -> Result<ProjectData, MissingOptionError> {
		let project_dir = self.project_dir.get_value().ok_or("project-dir")?;
		let initial_file = self.initial_file.get_value();
		let editor = self.editor.value.ok_or("editor")?.0;
		let fs_tree = self.virtual_fs.tree;
		let whitelist_all_envvars = self.whitelist_all_envvars.get_value().unwrap_or_default();
		let whitelist_envvars = self.whitelist_envvars.get_value().unwrap_or_default();
		let disable_sandbox = self.disable_sandbox.get_value().unwrap_or_default();

		let whitelist_envvars = if whitelist_all_envvars {
			EnvVarWhitelist::All
		} else {
			let os_string_list = whitelist_envvars.into_iter().map(Into::into).collect();
			EnvVarWhitelist::List(os_string_list)
		};

		Ok(ProjectData {
			command: editor.into_command(project_dir, initial_file.as_deref()),
			sandbox_params: (!disable_sandbox).then_some(SandboxParameters {
				envvar_whitelist: whitelist_envvars,
				fs_tree: fs_tree.remove_user_data(),
			}),
		})
	}
	fn parse_path(&mut self, path: impl AsRef<Path>, ctx: &mut ParseContext) -> ModResult<()> {
		let path = path.as_ref();

		if self.parsed_files.iter().any(|p| p == path) {
			return Ok(());
		}
		self.parsed_files.push(path.to_path_buf());

		let mut outlivers = (None, None);
		let parsed_contents = parse_lib::parse_toml_file(path, ctx.file_database, &mut outlivers)?;

		self.parse_table(&parsed_contents, ctx)?;
		Ok(())
	}
	fn parse_table(&mut self, table: &TomlTable, ctx: &mut ParseContext) -> ModResult<()> {
		let mut include_option = ArrayOption::new("include", false, |raw_value| {
			let value = raw_value.as_str()?;
			Self::canonicalize_include_path(value)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value.loc(), &err).into())
		});

		parse_lib::parse_table!(
			table => [
				include_option,
				self.project_dir,
				self.initial_file,
				self.editor,
				self.virtual_fs,
				self.whitelist_envvars,
				self.whitelist_all_envvars,
				self.disable_sandbox
			],
			docs-section: "PROJECT DATA FORMAT",
		)?;

		for include_path in include_option.get_value().unwrap_or_default() {
			self.parse_path(include_path, ctx)?;
		}
		Ok(())
	}
	fn canonicalize_include_path(path: &str) -> Result<PathBuf, CanonicalizationError> {
		let path = PathBuf::from(string_interpolation::resolve_placeholders(path, false)?);

		if path.is_absolute() {
			return Ok(path);
		};

		let mut matching_files = Vec::new();
		let skeld_data_dirs =
			crate::dirs::get_skeld_data_dirs().map_err(|err| CanonicalizationError {
				notes: vec![err.to_string()],
				..CanonicalizationError::main_message("could not determine the skeld data directories")
			})?;
		for data_root_dir in skeld_data_dirs {
			let include_root_dir = data_root_dir.join("include");
			let mut possible_file_path = include_root_dir.join(&path);
			possible_file_path.as_mut_os_string().push(".toml");
			if possible_file_path.exists() {
				matching_files.push(possible_file_path);
			}
		}

		if matching_files.is_empty() {
			let mut notes = vec![format!(
				"include files are searched in `<SKELD-DATA>/include`\n(see `{man_cmd}` for more information)",
				man_cmd = crate::error::get_manpage_cmd("FILES"),
			)];
			if path.extension().is_some_and(|ext| ext == "toml") {
				notes.push(format!(
					"Note that an extra `toml` extension is appended, so the file `{}.toml` is actually searched.",
					path.display()
				));
			}
			Err(CanonicalizationError {
				notes,
				..CanonicalizationError::main_message("include file not found")
			})
		} else if matching_files.len() > 1 {
			let matching_files_str = matching_files
				.iter()
				.map(|path| format!("- {}", path.display()))
				.collect::<Vec<_>>()
				.join("\n");
			Err(CanonicalizationError {
				labels: vec![CanonicalizationLabel::primary_without_span(
					"found multiple matching files",
				)],
				notes: vec![format!("matching files are:\n{matching_files_str}")],
				..CanonicalizationError::main_message("ambiguous include file")
			})
		} else {
			assert!(matching_files.len() == 1);
			Ok(matching_files.into_iter().next().unwrap())
		}
	}
}
#[derive(Clone)]
pub struct MissingOptionError(pub String);
impl<T: Into<String>> From<T> for MissingOptionError {
	fn from(value: T) -> Self {
		Self(value.into())
	}
}

#[derive(Clone)]
struct VirtualFSOption {
	tree: VirtualFSTree<parse_lib::Location>,
}
impl VirtualFSOption {
	fn new() -> Self {
		Self {
			tree: VirtualFSTree::new(),
		}
	}
}
impl parse_lib::ConfigOption for VirtualFSOption {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		let fs_entry_type;
		if key.name() == "whitelist-dev" {
			fs_entry_type = VirtualFSEntryType::AllowDev;
		} else if key.name() == "whitelist-rw" {
			fs_entry_type = VirtualFSEntryType::ReadWrite;
		} else if key.name() == "whitelist-ro" {
			fs_entry_type = VirtualFSEntryType::ReadOnly;
		} else if key.name() == "whitelist-ln" {
			fs_entry_type = VirtualFSEntryType::Symlink;
		} else if key.name() == "add-tmpfs" {
			fs_entry_type = VirtualFSEntryType::Tmpfs;
		} else {
			return Ok(false);
		}

		let mut patharray_option = ArrayOption::new(key.name(), false, |raw_value| {
			let value = raw_value.as_str()?;
			let parsed_value = canonicalize_path(value)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value.loc(), &err))?;
			Ok((parsed_value, raw_value.loc().clone()))
		});
		patharray_option.try_eat(key, value)?;
		for (path, loc) in patharray_option.get_value().unwrap_or_default() {
			match self.tree.add_path(path, fs_entry_type, loc) {
				Ok(()) => (),
				Err(FSTreeError::IllegalChildren {
					inner_path,
					invalid_child,
				}) => {
					let inner_path_label = inner_path
						.get_primary_label()
						.with_message("subpaths of symlink/tmpfs whitelists must not be whitelisted");
					let child_label = invalid_child
						.get_secondary_label()
						.with_message("but here a subpath is whitelisted");
					let diag = parse_lib::Diagnostic::new(parse_lib::Severity::Error)
						.with_message("subpath of symlink/tmpfs is whitelisted")
						.with_labels(vec![inner_path_label, child_label]);
					return Err(diag.into());
				}
				Err(FSTreeError::ConflictingEntries(first, second)) => {
					let first_label = first
						.get_primary_label()
						.with_message("path whitelisted here");
					let second_label = second.get_secondary_label().with_message("and here again");
					let diag = parse_lib::Diagnostic::new(parse_lib::Severity::Error)
						.with_message("conflicting whitelists")
						.with_labels(vec![first_label, second_label]);
					return Err(diag.into());
				}
			}
		}

		Ok(true)
	}
}

#[derive(Clone)]
struct EditorCommand {
	cmd_with_file: Vec<String>,
	cmd_without_file: Vec<String>,
	detach: bool,
}
impl EditorCommand {
	fn into_command(self, project_dir: PathBuf, initial_file: Option<&str>) -> Command {
		let command: Vec<_> = if let Some(initial_file) = initial_file {
			self
				.cmd_with_file
				.into_iter()
				.map(|arg| arg.replace("$(FILE)", initial_file))
				.collect()
		} else {
			self.cmd_without_file.into_iter().collect()
		};

		let mut command_iter = command.into_iter();
		Command {
			program: command_iter.next().expect("command should not be empty"),
			args: command_iter.collect(),
			working_dir: project_dir,
			detach: self.detach,
		}
	}
}
#[derive(Clone)]
struct EditorCommandOption {
	value: Option<(EditorCommand, parse_lib::Location)>,
}
impl EditorCommandOption {
	fn new() -> Self {
		Self { value: None }
	}
}
impl parse_lib::ConfigOption for EditorCommandOption {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		if key.name() != "editor" {
			return Ok(false);
		}
		if let Some((_, prev_loc)) = &self.value {
			return Err(diagnostics::multiple_definitions(key.loc(), prev_loc, "editor").into());
		}
		let table = value.as_table()?;

		let mut cmd_with_file = ArrayOption::new("cmd-with-file", false, |raw_value| {
			let value = raw_value.as_str()?;
			string_interpolation::resolve_placeholders(value, true)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value.loc(), &err).into())
		});
		let mut cmd_without_file = ArrayOption::new("cmd-without-file", false, |raw_value| {
			let value = raw_value.as_str()?;
			string_interpolation::resolve_placeholders(value, false)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value.loc(), &err).into())
		});
		let mut detach = BoolOption::new("detach");

		let docs_section = "PROJECT DATA FORMAT";
		parse_lib::parse_table!(
			&table => [cmd_with_file, cmd_without_file, detach],
			docs-section: docs_section,
		)?;
		let cmd_with_file = cmd_with_file
			.get_value_with_loc()
			.ok_or_else(|| diagnostics::missing_option(key.loc(), "cmd-with-file", docs_section))?;
		let cmd_without_file = cmd_without_file
			.get_value_with_loc()
			.ok_or_else(|| diagnostics::missing_option(key.loc(), "cmd-without-file", docs_section))?;
		let detach = detach
			.get_value()
			.ok_or_else(|| diagnostics::missing_option(key.loc(), "detach", docs_section))?;

		let diagnostics_empty_command = |loc: parse_lib::Location| {
			let label = loc
				.get_primary_label()
				.with_message("command must not be empty");
			parse_lib::Diagnostic::new(parse_lib::Severity::Error)
				.with_message("empty editor command")
				.with_labels(vec![label])
		};
		if cmd_with_file.0.is_empty() {
			return Err(diagnostics_empty_command(cmd_with_file.1).into());
		}
		if cmd_without_file.0.is_empty() {
			return Err(diagnostics_empty_command(cmd_without_file.1).into());
		}

		let editor_cmd = EditorCommand {
			cmd_with_file: cmd_with_file.0,
			cmd_without_file: cmd_without_file.0,
			detach,
		};
		self.value = Some((editor_cmd, key.loc().clone()));
		Ok(true)
	}
}

fn canonicalize_path(path: &str) -> Result<PathBuf, CanonicalizationError> {
	let substituted_path_str = string_interpolation::resolve_placeholders(path, false)?;
	let substituted_path = PathBuf::from(&substituted_path_str);

	if substituted_path.is_relative() {
		let mut notes = Vec::new();
		if path != substituted_path_str {
			notes.push(format!(
				"after the placeholders have been resolved: `{substituted_path_str}`"
			));
		}
		return Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_without_span(
				"this path must be absolute",
			)],
			notes,
			..CanonicalizationError::main_message("unallowed relative path")
		});
	}

	Ok(substituted_path)
}
