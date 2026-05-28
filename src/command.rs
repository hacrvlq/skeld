use std::{
	error::Error,
	fs::{self, File},
	io,
	os::fd::IntoRawFd as _,
	path::{Path, PathBuf},
	process::{Command as OsCommand, ExitCode, ExitStatus},
	time::Duration,
};

use nix::{
	sys::wait::{self, WaitStatus},
	unistd,
};

#[derive(Clone, Debug)]
pub struct Command {
	pub program: String,
	pub args: Vec<String>,
	// Must be an absolute path.
	pub working_dir: Option<PathBuf>,
	pub detach: bool,
}

impl Command {
	pub fn run(&self) -> Result<ExitCode, Box<dyn Error>> {
		if self.detach {
			detach_from_tty()?;
		}

		let mut cmd = OsCommand::new(&self.program);
		cmd.args(&self.args);
		if let Some(working_dir) = &self.working_dir {
			cmd.current_dir(working_dir);
		}

		let mut child = cmd
			.spawn()
			.map_err(|err| format!("Failed to execute command `{}`: {err}", &self.program))?;

		if self.detach {
			Ok(ExitCode::SUCCESS)
		} else {
			let child_status = child.wait().unwrap();
			Ok(forward_child_exit_status(child_status))
		}
	}
}

pub fn forward_child_exit_status(status: ExitStatus) -> ExitCode {
	if let Some(code) = status.code() {
		(code as u8).into()
	} else if status.success() {
		ExitCode::SUCCESS
	} else {
		ExitCode::FAILURE
	}
}

// Detach this process from the controlling terminal and redirect stdout/stderr
// to a logfile.
pub fn detach_from_tty() -> Result<(), String> {
	let logdir = crate::dirs::get_skeld_state_dir()
		.map_err(|err| format!("Failed to determine the skeld state directory:\n  {err}"))?;
	fs::create_dir_all(&logdir).map_err(|err| {
		format!(
			"Failed to create the skeld state directory `{}`:\n  {err}",
			logdir.display()
		)
	})?;

	remove_old_logfiles(&logdir);

	let (logfile_path, logfile) =
		create_logfile(logdir).map_err(|err| format!("Failed to create a logfile: {err}"))?;

	// SAFETY: This program isn't multithreaded.
	match unsafe { unistd::fork() }.unwrap() {
		unistd::ForkResult::Parent { child } => match wait::waitpid(child, None).unwrap() {
			WaitStatus::Exited(_, code) => std::process::exit(code),
			WaitStatus::Signaled(_, _, _) => std::process::exit(1),
			status => panic!("Got unexpected wait status: {status:?}"),
		},
		unistd::ForkResult::Child => (),
	}

	println!(
		concat!(
			"NOTE: Detaching from terminal;\n",
			"      further output will be redirected to `{}`",
		),
		logfile_path.display()
	);
	unistd::close(0).unwrap();
	unistd::dup2_stdout(&logfile).unwrap();
	unistd::dup2_stderr(&logfile).unwrap();
	// Leak the file descriptor.
	let _ = logfile.into_raw_fd();

	unistd::setsid().unwrap();

	// SAFETY: This program isn't multithreaded.
	match unsafe { unistd::fork() }.unwrap() {
		unistd::ForkResult::Parent { .. } => std::process::exit(0),
		unistd::ForkResult::Child => (),
	}

	Ok(())
}
// Remove logfiles older than 24h, errors are silently ignored.
fn remove_old_logfiles(logdir: impl AsRef<Path>) {
	let Ok(dir_iter) = fs::read_dir(logdir) else {
		return;
	};

	dir_iter
		.filter_map(Result::ok)
		.filter(|dir_entry| dir_entry.path().extension().is_some_and(|ext| ext == "log"))
		.filter(|dir_entry| {
			let elapsed_time = dir_entry
				.metadata()
				.ok()
				.and_then(|metadata| metadata.accessed().ok())
				.and_then(|mtime| mtime.elapsed().ok());
			let Some(elapsed_time) = elapsed_time else {
				// Remove the file in case of an error.
				return false;
			};
			elapsed_time > Duration::from_secs(60 * 60 * 24)
		})
		.for_each(|dir_entry| {
			let _ = fs::remove_file(dir_entry.path());
		});
}
fn create_logfile(logdir: impl AsRef<Path>) -> io::Result<(PathBuf, File)> {
	let logdir = logdir.as_ref();

	for i in 1..1_440 {
		let possible_path = logdir.join(format!("skeld.{i}.log"));
		match File::create_new(&possible_path) {
			Ok(file) => return Ok((possible_path, file)),
			Err(err) if err.kind() == io::ErrorKind::AlreadyExists => (),
			Err(other_err) => return Err(other_err),
		}
	}

	Err(io::Error::other("all logfile names are occupied"))
}
