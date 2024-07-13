use std::{
	error::Error,
	path::{Path, PathBuf},
	process::ExitCode,
};

use crate::sandbox::{Command, SandboxParameters};

#[derive(Clone)]
pub struct ProjectData {
	pub project_dir: PathBuf,
	pub initial_file: Option<String>,
	pub editor: EditorCommand,
	pub auto_nixshell: bool,
	pub sandbox_params: SandboxParameters,
}
#[derive(Clone, Debug)]
pub struct EditorCommand {
	pub cmd_with_file: Vec<String>,
	pub cmd_without_file: Vec<String>,
	pub detach: bool,
}

impl ProjectData {
	pub fn open(mut self) -> Result<ExitCode, Box<dyn Error>> {
		// NOTE: if the user gives the project directory higher permsission
		//       or tmpfs/symlinks it, 'add_path' returns an error,
		//       but it should be ignored
		_ = self.sandbox_params.fs_tree.add_path(
			&self.project_dir,
			crate::sandbox::VirtualFSEntryType::ReadWrite,
			(),
		);

		let sandbox_cmd = self
			.editor
			.get_command(self.project_dir.clone(), self.initial_file);
		let use_nix_shell = self.auto_nixshell && detect_nix_shell_file(&self.project_dir);
		let sandbox_cmd = if use_nix_shell {
			wrap_cmd_with_nix_shell(sandbox_cmd)?
		} else {
			sandbox_cmd
		};

		let exit_code = self.sandbox_params.run_cmd(sandbox_cmd)?;
		Ok(exit_code)
	}
}
impl EditorCommand {
	fn get_command(self, working_dir: PathBuf, initial_file: Option<String>) -> Command {
		let command = if let Some(initial_file) = initial_file {
			self
				.cmd_with_file
				.into_iter()
				.map(|arg| arg.replace("$(FILE)", &initial_file).into())
				.collect()
		} else {
			self
				.cmd_without_file
				.into_iter()
				.map(|path| path.into())
				.collect()
		};

		Command {
			cmd: command,
			working_dir,
			detach: self.detach,
		}
	}
}
fn detect_nix_shell_file(project_path: impl AsRef<Path>) -> bool {
	let project_path = project_path.as_ref();
	project_path.join("shell.nix").exists() || project_path.join("default.nix").exists()
}
fn wrap_cmd_with_nix_shell(cmd: Command) -> Result<Command, Box<dyn Error>> {
	let escaped_cmd = cmd
		.cmd
		.iter()
		.map(|os_str| {
			os_str.to_str().ok_or(format!(
				"argument of command is invalid UTF-8 (`{}`)",
				os_str.to_string_lossy()
			))
		})
		.collect::<Result<Vec<_>, String>>()?
		.into_iter()
		.map(bash_string_escape)
		.collect::<Vec<_>>();
	let wrapped_cmd = vec![
		"nix-shell".to_string(),
		"--command".to_string(),
		escaped_cmd.join(" "),
	];

	Ok(Command {
		cmd: wrapped_cmd.into_iter().map(|str| str.into()).collect(),
		..cmd
	})
}
//TODO: proper escaping
fn bash_string_escape(str: &str) -> String {
	let escaped_str = str.replace('\\', "\\\\").replace('\'', "\\'");
	format!("$'{}'", escaped_str)
}
