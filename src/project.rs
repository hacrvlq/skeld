use std::{error::Error, path::PathBuf, process::ExitCode};

use crate::{command::Command, sandbox::SandboxParameters};

#[derive(Clone)]
pub struct ProjectData {
	pub project_dir: PathBuf,
	pub initial_file: Option<String>,
	pub editor: EditorCommand,
	pub sandbox_params: SandboxParameters,
	pub disable_sandbox: bool,
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

		let project_cmd = self
			.editor
			.get_command(self.project_dir.clone(), self.initial_file);
		if self.disable_sandbox {
			project_cmd.run()
		} else {
			self.sandbox_params.run_cmd(project_cmd)
		}
	}
}
impl EditorCommand {
	fn get_command(self, working_dir: PathBuf, initial_file: Option<String>) -> Command {
		let command: Vec<_> = if let Some(initial_file) = initial_file {
			self
				.cmd_with_file
				.into_iter()
				.map(|arg| arg.replace("$(FILE)", &initial_file))
				.collect()
		} else {
			self.cmd_without_file.into_iter().collect()
		};

		let mut command_iter = command.into_iter();
		Command {
			program: command_iter.next().expect("command should not be empty"),
			args: command_iter.collect(),
			working_dir,
			detach: self.detach,
		}
	}
}
