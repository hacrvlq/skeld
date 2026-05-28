use std::{error::Error, path::PathBuf, process::ExitCode};

use crate::{
	command::Command,
	parsing::{ParseContext, RawProjectData},
	sandbox::{self, SandboxParameters},
};

#[derive(Clone, Debug)]
pub struct ProjectFileData {
	pub name: String,
	// If `keybind` is `None`, an automatically determined keybinding will be used.
	pub keybind: Option<String>,
	pub project_data_file: ProjectDataFile,
}
#[derive(Clone, Debug)]
pub struct ProjectDataFile(pub PathBuf);
impl ProjectDataFile {
	pub fn load(
		self,
		initial_data: RawProjectData,
		ctx: &mut ParseContext,
	) -> crate::GenericResult<ProjectData> {
		ctx.parse_project_file(self.0, initial_data)
	}
}

#[derive(Clone, Debug)]
pub struct ProjectData {
	pub command: Command,
	pub sandbox_params: Option<SandboxParameters>,
}
impl ProjectData {
	pub fn open(self) -> Result<ExitCode, Box<dyn Error>> {
		let Some(mut sandbox_params) = self.sandbox_params else {
			return self.command.run();
		};

		if let Some(project_dir) = &self.command.working_dir {
			// NOTE: If the user gives the project directory higher permsission or
			// tmpfs/symlinks it, `add_path` returns an error, which should be ignored
			_ = sandbox_params
				.fs_tree
				.add_path(project_dir, sandbox::VirtualFSEntryType::ReadWrite, ());
		}
		sandbox::run_sandboxed(self.command, &sandbox_params)
	}
}
