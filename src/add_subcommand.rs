use std::{
	env,
	error::Error,
	ffi::{OsStr, OsString},
	fs::{self, File},
	io::{self, Write as _},
	os::unix::ffi::OsStringExt as _,
	path::{Path, PathBuf},
	process::Command,
};

use crate::{dirs, AddArgs};

type ModResult<T> = Result<T, Box<dyn Error>>;

pub fn run(args: AddArgs) -> ModResult<()> {
	let project_path = args.project_path.canonicalize().map_err(|err| {
		format!(
			"Failed to read path `{}`: {err}",
			args.project_path.display()
		)
	})?;

	let project_name = if let Some(name) = &args.project_name {
		name
	} else {
		get_project_name_from_path(&project_path)
			.ok_or("Could not determine project name from path, use option '--name' instead")?
	};

	let project_file_contents = if project_path.is_file() {
		let project_dir = project_path.parent().unwrap();
		let project_file = project_path.file_name().unwrap();
		format!(
			"project-dir = {}\ninitial-file = {}",
			toml_string_escape(normalize_path_prefix(project_dir).as_os_str())?,
			toml_string_escape(project_file)?
		)
	} else {
		format!(
			"project-dir = {}",
			toml_string_escape(normalize_path_prefix(&project_path).as_os_str())?
		)
	};

	let projects_dir = dirs::get_skeld_data_dir()?.join("projects");
	fs::create_dir_all(&projects_dir)
		.map_err(|err| format!("Failed to create skeld projects directory: {err}"))?;

	let project_filename = projects_dir.join(format!("{project_name}.toml"));
	let mut project_file = File::create_new(&project_filename).map_err(|err| {
		if err.kind() == io::ErrorKind::AlreadyExists {
			"A project with the same name already exists, use option '--name' to use a different name"
				.to_string()
		} else {
			format!("Failed to create project file: {err}")
		}
	})?;
	write!(project_file, "{project_file_contents}")
		.map_err(|err| format!("Failed to write project file: {err}"))?;

	launch_editor(&project_filename)?;

	Ok(())
}
fn get_project_name_from_path(path: &Path) -> Option<&str> {
	let basename = path.file_name()?.to_str()?;
	let basename = basename.strip_prefix('.').unwrap_or(basename);

	let project_name = if path.is_file() {
		&basename[..basename.find('.').unwrap_or(basename.len())]
	} else {
		basename
	};

	assert!(!project_name.is_empty());
	Some(project_name)
}
// use known path prefixes like '~'
fn normalize_path_prefix(path: impl AsRef<Path>) -> PathBuf {
	let path = path.as_ref();

	let handle_prefix = |prefix: Option<PathBuf>, replacement: &str| {
		let replacement: PathBuf = replacement.into();
		let prefix = prefix?;

		if path.starts_with(&prefix) {
			Some(replacement.join(path.strip_prefix(&prefix).unwrap()))
		} else {
			None
		}
	};

	if let Some(path) = handle_prefix(dirs::get_xdg_config_dir().ok(), "$(CONFIG)") {
		path
	} else if let Some(path) = handle_prefix(dirs::get_xdg_cache_dir().ok(), "$(CACHE)") {
		path
	} else if let Some(path) = handle_prefix(dirs::get_xdg_data_dir().ok(), "$(DATA)") {
		path
	} else if let Some(path) = handle_prefix(dirs::get_xdg_state_dir().ok(), "$(STATE)") {
		path
	} else if let Some(path) = handle_prefix(dirs::get_home_dir().ok(), "~") {
		path
	} else {
		path.to_path_buf()
	}
}
//TODO: allow all UTF-8
fn toml_string_escape(str: &OsStr) -> ModResult<String> {
	let escaped_str = str
		.to_str()
		.filter(|str| str.chars().all(|ch| ch.is_ascii_graphic() || ch == ' '))
		.ok_or("Can only handle printable ASCII characters in paths")?
		.replace('\\', "\\\\")
		.replace('"', "\\\"");
	Ok(format!("\"{escaped_str}\""))
}

fn launch_editor(file: impl AsRef<Path>) -> ModResult<()> {
	let mut editor_cmd = get_editor();
	editor_cmd.push(" ");
	editor_cmd.push(shell_string_escape(file.as_ref().as_os_str()));

	Command::new("sh")
		.arg("-c")
		.arg(editor_cmd)
		.spawn()
		.map_err(|err| format!("Failed to launch editor: {err}"))?
		.wait()
		.map_err(|err| format!("Failed to wait for editor: {err}"))?;

	Ok(())
}
// inspired by git's process of determining the editor
fn get_editor() -> OsString {
	let terminal_is_dumb = !env::var_os("TERM").is_some_and(|val| val != "dumb");

	match env::var_os("VISUAL") {
		Some(val) if !val.is_empty() && !terminal_is_dumb => return val,
		_ => (),
	}
	match env::var_os("EDITOR") {
		Some(val) if !val.is_empty() => return val,
		_ => (),
	}

	if terminal_is_dumb {
		"vi -e".into()
	} else {
		"vi".into()
	}
}
// escape string for POSIX-compliant shells
fn shell_string_escape(str: &OsStr) -> OsString {
	let mut escaped_bytes = Vec::new();
	escaped_bytes.push(b'"');
	escaped_bytes.extend(str.as_encoded_bytes().iter().flat_map(|&byte| {
		if byte == b'$' {
			vec![b'\\', b'$']
		} else if byte == b'`' {
			vec![b'\\', b'`']
		} else if byte == b'\\' {
			vec![b'\\', b'\\']
		} else if byte == b'"' {
			vec![b'\\', b'"']
		} else {
			vec![byte]
		}
	}));
	escaped_bytes.push(b'"');
	OsString::from_vec(escaped_bytes)
}
