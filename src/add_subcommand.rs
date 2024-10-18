use std::{
	env,
	error::Error,
	ffi::{OsStr, OsString},
	fs::{self, File},
	io::{self, Write as _},
	os::unix::ffi::OsStringExt as _,
	path::{Path, PathBuf},
	process::{self, Command},
};

use crate::{dirs, AddArgs};

type ModResult<T> = Result<T, Box<dyn Error>>;

pub fn run(args: AddArgs) -> ModResult<()> {
	let project_path = args.project_path.canonicalize().map_err(|err| {
		format!(
			"Failed to canonicalize the project path `{}`: {err}",
			args.project_path.display()
		)
	})?;

	let project_name = if let Some(name) = &args.project_name {
		name
	} else {
		get_project_name_from_path(&project_path).ok_or(concat!(
			"Failed to determine a project name from the path.\n",
			"  NOTE: Use the option '--name' to specify a name."
		))?
	};

	let project_file_contents = if project_path.is_file() {
		let project_dir = normalize_path_prefix(project_path.parent().unwrap());
		let project_dir = project_dir.to_str().ok_or_else(|| {
			format!(
				concat!(
					"Failed to make a toml string with the specified project directory,\n",
					"because it contains invalid UTF-8: `{}`"
				),
				project_dir.display()
			)
		})?;

		let project_file = project_path.file_name().unwrap();
		let project_file = project_file.to_str().ok_or_else(|| {
			format!(
				concat!(
					"Failed to make a toml string with the specified project name,\n",
					"because it contains invalid UTF-8: `{}`"
				),
				project_file.to_string_lossy(),
			)
		})?;

		format!(
			"project-dir = {}\ninitial-file = {}",
			toml_string_escape(project_dir),
			toml_string_escape(project_file)
		)
	} else {
		let project_dir = normalize_path_prefix(&project_path);
		let project_dir = project_dir.to_str().ok_or_else(|| {
			format!(
				concat!(
					"Failed to make a toml string with the specified project path,\n",
					"because it contains invalid UTF-8: `{}`"
				),
				project_dir.display()
			)
		})?;

		format!("project-dir = {}", toml_string_escape(project_dir))
	};

	let projects_dir = dirs::get_skeld_data_dir()
		.map_err(|err| format!("Failed to determine the skeld data directory:\n  {err}"))?
		.join("projects");
	fs::create_dir_all(&projects_dir).map_err(|err| {
		format!(
			"Failed to create the skeld projects directory `{}`:\n  {err}",
			projects_dir.display()
		)
	})?;

	let project_filename = projects_dir.join(format!("{project_name}.toml"));
	let mut project_file = File::create_new(&project_filename).map_err(|err| {
		if err.kind() == io::ErrorKind::AlreadyExists {
			concat!(
				"Failed to add the project, because a project with the same name already exists.\n",
				"  NOTE: Use option '--name' to specify a different name."
			)
			.to_string()
		} else {
			format!(
				"Failed to create the project file `{}`:\n  {err}",
				project_filename.display()
			)
		}
	})?;
	write!(project_file, "{project_file_contents}").unwrap();

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
fn toml_string_escape(str: &str) -> String {
	let escaped_str = str
		.chars()
		.map(|char| match char {
			'\x08' => "\\b".to_string(),
			'\t' => "\\t".to_string(),
			'\n' => "\\n".to_string(),
			'\x0c' => "\\f".to_string(),
			'\r' => "\\r".to_string(),
			'\\' => "\\\\".to_string(),
			'"' => "\\\"".to_string(),
			ch if ch.is_ascii_graphic() || ch == ' ' => ch.to_string(),
			ch => format!("\\U{:08X}", ch as u32),
		})
		.collect::<String>();
	format!("\"{escaped_str}\"")
}

fn launch_editor(file: impl AsRef<Path>) -> ModResult<()> {
	let mut editor_cmd = get_editor();
	editor_cmd.push(" ");
	editor_cmd.push(shell_string_escape(file.as_ref().as_os_str()));

	let editor_output = Command::new("sh")
		.arg("-c")
		.arg(&editor_cmd)
		// NOTE: This won't work if the editor uses stderr to display its tui,
		//       but that should be unlikely.
		.stderr(process::Stdio::piped())
		.spawn()
		.expect("failed to execute `sh`")
		.wait_with_output()
		.unwrap();

	if !editor_output.status.success() {
		let editor_stderr = String::from_utf8_lossy(&editor_output.stderr);
		let trimmed_stderr = editor_stderr
			.trim()
			.strip_prefix("sh: line 1: ")
			.unwrap_or(&editor_stderr);
		return Err(
			format!(
				"Failed to execute the editor command `{}`:\n  {}",
				editor_cmd.to_string_lossy(),
				trimmed_stderr.replace('\n', "\n  ")
			)
			.into(),
		);
	}

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
	escaped_bytes.extend(str.as_encoded_bytes().iter().flat_map(|&byte| match byte {
		b'$' => vec![b'\\', b'$'],
		b'`' => vec![b'\\', b'`'],
		b'\\' => vec![b'\\', b'\\'],
		b'"' => vec![b'\\', b'"'],
		_ => vec![byte],
	}));
	escaped_bytes.push(b'"');
	OsString::from_vec(escaped_bytes)
}
