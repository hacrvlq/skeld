use std::path::{Path, PathBuf};

use super::{
	lib::{
		self as parse_lib, diagnostics, ArrayOption, BoolOption, PathBufOption, StringOption, TomlKey,
		TomlTable, TomlValue,
	},
	path, ModResult, ParseContext,
};
use crate::{
	project::{EditorCommand, ProjectData},
	sandbox::{EnvVarWhitelist, FSTreeError, SandboxParameters, VirtualFSEntryType, VirtualFSTree},
};

#[derive(Clone, Debug)]
pub enum ProjectDataFuture {
	Project(PathBuf),
	Bookmark(PathBuf),
}
impl ProjectDataFuture {
	pub fn load(
		self,
		parse_state: PrelimParseState,
		ctx: &mut ParseContext,
	) -> ModResult<ProjectData> {
		match self {
			Self::Project(path) => Self::parse_project_data_file(path, parse_state, ctx),
			Self::Bookmark(path) => Self::parse_bookmark_file_stage2(path, parse_state, ctx),
		}
	}
	fn parse_project_data_file(
		path: impl AsRef<Path>,
		mut parse_state: PrelimParseState,
		ctx: &mut ParseContext,
	) -> ModResult<ProjectData> {
		let mut outlivers = (None, None);
		let parsed_contents = parse_lib::parse_toml_file(path, &mut ctx.file_database, &mut outlivers)?;
		parse_state.parse_table(&parsed_contents, ctx)?;

		let project_data = parse_state
			.into_project_data()
			.map_err(|missing| diagnostics::missing_option(parsed_contents.loc(), &missing))?;
		Ok(project_data)
	}
	fn parse_bookmark_file_stage2(
		path: impl AsRef<Path>,
		parse_state: PrelimParseState,
		ctx: &mut ParseContext,
	) -> ModResult<ProjectData> {
		let mut outlivers = (None, None);
		let parsed_contents =
			parse_lib::parse_toml_file(path.as_ref(), &mut ctx.file_database, &mut outlivers)?;

		let mut name = StringOption::new("name");
		let mut keybind = StringOption::new("keybind");
		let mut project_data = ProjectDataOption::new("project", parse_state, ctx);
		parse_lib::parse_table!(&parsed_contents => [name, keybind, project_data])?;

		let project_data = project_data
			.get_value()
			.into_project_data()
			.map_err(|missing| diagnostics::missing_option(parsed_contents.loc(), &missing))?;
		Ok(project_data)
	}
}

pub struct ProjectDataOption<'a> {
	name: String,
	value: PrelimParseState,
	ctx: &'a mut ParseContext,
}
impl<'a> ProjectDataOption<'a> {
	pub fn new(name: &str, initial_state: PrelimParseState, ctx: &'a mut ParseContext) -> Self {
		Self {
			name: name.to_string(),
			value: initial_state,
			ctx,
		}
	}
	pub fn get_value(self) -> PrelimParseState {
		self.value
	}
}
impl parse_lib::ConfigOption for ProjectDataOption<'_> {
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
pub struct PrelimParseState {
	project_dir: PathBufOption,
	initial_file: StringOption,
	editor: EditorCommandOption,
	virtual_fs: VirtualFSOption,
	whitelists_envvars: ArrayOption<String>,
	whitelist_all_envvars: BoolOption,
	auto_nixshell: BoolOption,
	disable_sandbox: BoolOption,

	parsed_files: Vec<PathBuf>,
}
impl PrelimParseState {
	pub fn empty() -> Self {
		Self {
			project_dir: PathBufOption::new("project-dir", |str| Ok(path::canonicalize_path(str)?)),
			initial_file: StringOption::new_with_canonicalization("initial-file", |str| {
				Ok(path::substitute_placeholder(str, false)?)
			}),
			editor: EditorCommandOption::new(),
			virtual_fs: VirtualFSOption::new(),
			whitelists_envvars: ArrayOption::new("whitelists-envvar", true, |value| {
				let value = value.as_str()?;
				Ok(value.to_string())
			}),
			whitelist_all_envvars: BoolOption::new("whitelist-all-envvars"),
			auto_nixshell: BoolOption::new("auto-nixshell"),
			disable_sandbox: BoolOption::new("no-sandbox"),

			parsed_files: Vec::new(),
		}
	}
	// if a required config option is missing, the name of this option is returned as an error
	fn into_project_data(self) -> Result<ProjectData, String> {
		let project_dir = self.project_dir.get_value().ok_or("project-dir")?;
		let initial_file = self.initial_file.get_value();
		let editor = self.editor.value.ok_or("editor")?.0;
		let fs_tree = self.virtual_fs.tree;
		let whitelist_all_envvars = self.whitelist_all_envvars.get_value().unwrap_or_default();
		let whitelist_envvars = self.whitelists_envvars.get_value().unwrap_or_default();
		let auto_nixshell = self.auto_nixshell.get_value().unwrap_or_default();
		let disable_sandbox = self.disable_sandbox.get_value().unwrap_or_default();

		let whitelist_envvars = if whitelist_all_envvars {
			EnvVarWhitelist::All
		} else {
			let os_string_list = whitelist_envvars.into_iter().map(Into::into).collect();
			EnvVarWhitelist::List(os_string_list)
		};
		Ok(ProjectData {
			project_dir,
			auto_nixshell,
			disable_sandbox,
			initial_file,
			editor,
			sandbox_params: SandboxParameters {
				envvar_whitelist: whitelist_envvars,
				fs_tree: fs_tree.remove_user_data(),
			},
		})
	}
	fn parse_path(&mut self, path: impl AsRef<Path>, ctx: &mut ParseContext) -> ModResult<()> {
		let path = path.as_ref();

		if self.parsed_files.iter().any(|p| p == path) {
			return Ok(());
		}
		self.parsed_files.push(path.to_path_buf());

		let mut outlivers = (None, None);
		let parsed_contents = parse_lib::parse_toml_file(path, &mut ctx.file_database, &mut outlivers)?;

		self.parse_table(&parsed_contents, ctx)?;
		Ok(())
	}
	fn parse_table(&mut self, table: &TomlTable, ctx: &mut ParseContext) -> ModResult<()> {
		let mut include_option = ArrayOption::new("include", false, |raw_value| {
			let value = raw_value.as_str()?;
			path::canonicalize_include_path(value)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value, &err).into())
		});

		parse_lib::parse_table!(table => [
			include_option,
			self.project_dir,
			self.initial_file,
			self.editor,
			self.virtual_fs,
			self.whitelists_envvars,
			self.whitelist_all_envvars,
			self.auto_nixshell,
			self.disable_sandbox
		])?;

		for include_path in include_option.get_value().unwrap_or_default() {
			self.parse_path(include_path, ctx)?;
		}
		Ok(())
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
		if key.name() == "whitelists-dev" {
			fs_entry_type = VirtualFSEntryType::AllowDev;
		} else if key.name() == "whitelists-rw" {
			fs_entry_type = VirtualFSEntryType::ReadWrite;
		} else if key.name() == "whitelists-ro" {
			fs_entry_type = VirtualFSEntryType::ReadOnly;
		} else if key.name() == "whitelists-ln" {
			fs_entry_type = VirtualFSEntryType::Symlink;
		} else if key.name() == "add-tmpfs" {
			fs_entry_type = VirtualFSEntryType::Tmpfs;
		} else {
			return Ok(false);
		}

		let mut patharray_option = ArrayOption::new(key.name(), false, |raw_value| {
			let value = raw_value.as_str()?;
			let parsed_value = path::canonicalize_path(value)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value, &err))?;
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
						.with_message("subpaths of symlink/tmpfs whitelist must not be whitelisted");
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
			path::substitute_placeholder(value, true)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value, &err).into())
		});
		let mut cmd_without_file = ArrayOption::new("cmd-without-file", false, |raw_value| {
			let value = raw_value.as_str()?;
			path::substitute_placeholder(value, false)
				.map_err(|err| diagnostics::failed_canonicalization(raw_value, &err).into())
		});
		let mut detach = BoolOption::new("detach");

		parse_lib::parse_table!(&table => [cmd_with_file, cmd_without_file, detach])?;
		let cmd_with_file = cmd_with_file
			.get_value_with_loc()
			.ok_or_else(|| diagnostics::missing_option(key.loc(), "cmd-with-file"))?;
		let cmd_without_file = cmd_without_file
			.get_value_with_loc()
			.ok_or_else(|| diagnostics::missing_option(key.loc(), "cmd-without-file"))?;
		let detach = detach
			.get_value()
			.ok_or_else(|| diagnostics::missing_option(key.loc(), "detach"))?;

		let diagnostics_empty_command = |loc: parse_lib::Location| {
			let label = loc
				.get_primary_label()
				.with_message("command cannot be empty");
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
