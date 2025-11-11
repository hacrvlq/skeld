use std::{error::Error, path::PathBuf, process::ExitCode};

use crate::{
	command::Command,
	parsing::{ParseContext, RawProjectData},
	sandbox::SandboxParameters,
};

#[derive(Clone, Debug)]
pub struct ProjectFileData {
	pub name: String,
	// if 'keybind' is 'None', an automatically determined keybinding will be used
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
			// NOTE: if the user gives the project directory higher permsission
			//       or tmpfs/symlinks it, 'add_path' returns an error,
			//       but it should be ignored
			_ = sandbox_params.fs_tree.add_path(
				project_dir,
				crate::sandbox::VirtualFSEntryType::ReadWrite,
				(),
			);
		}
		sandbox_params.run_cmd(self.command)
	}
}
