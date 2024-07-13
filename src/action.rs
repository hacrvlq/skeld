use std::{
	error::Error,
	process::{Command as OsCommand, ExitCode},
};

use nix::unistd;

use crate::parse::{Error as ParseError, ParseContext, PrelimParseState, ProjectDataFuture};

#[derive(Clone, Debug)]
pub enum Action {
	Run(Command),
	OpenProject(ProjectDataFuture),
}
impl Action {
	pub fn execute(
		self,
		parse_state: PrelimParseState,
		ctx: &mut ParseContext,
	) -> Result<ExitCode, ParseError> {
		match self {
			Action::Run(cmd) => cmd.run().map_err(|err| err.to_string().into()),
			Action::OpenProject(project) => {
				let project_result = project.load(parse_state, ctx)?;
				project_result.open().map_err(|err| err.to_string().into())
			}
		}
	}
}

//TODO: make project's expressive enough to also handle this concept
#[derive(Clone, Debug)]
pub struct Command {
	pub command: Vec<String>,
	pub detach: bool,
}
impl Command {
	fn run(self) -> Result<ExitCode, Box<dyn Error>> {
		if self.command.is_empty() {
			return Ok(ExitCode::SUCCESS);
		}
		let cmd = self.command[0].clone();
		let cmd_args = self.command.into_iter().skip(1);

		if self.detach {
			unistd::daemon(false, false).unwrap();
		}

		let mut child = OsCommand::new(&cmd)
			.args(cmd_args)
			.spawn()
			.map_err(|err| format!("Failed to execute command `{cmd}`:{err}"))?;

		let exit_status = child
			.wait()
			.map_err(|err| format!("Failed to wait for command: {err}"))?;

		if let Some(code) = exit_status.code() {
			Ok((code as u8).into())
		} else if exit_status.success() {
			Ok(ExitCode::SUCCESS)
		} else {
			Ok(ExitCode::FAILURE)
		}
	}
}
