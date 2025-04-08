use std::{
	cmp::Ordering,
	env,
	error::Error,
	ffi::OsString,
	fs::{self, File},
	io,
	os::fd::IntoRawFd as _,
	path::{Component as PathComponents, Path, PathBuf},
	process::{Command as OsCommand, ExitCode, ExitStatus},
	time::Duration,
};

use nix::{errno::Errno, unistd};
use seccompiler::{
	BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
	SeccompRule, TargetArch as SeccompArch,
};

#[derive(Clone)]
pub struct SandboxParameters {
	pub fs_tree: VirtualFSTree<()>,
	pub envvar_whitelist: EnvVarWhitelist,
}
#[derive(Clone)]
pub enum EnvVarWhitelist {
	All,
	List(Vec<OsString>),
}
#[derive(Clone, Debug)]
pub struct Command {
	pub cmd: Vec<String>,
	pub working_dir: PathBuf,
	pub detach: bool,
}
impl SandboxParameters {
	pub fn run_cmd(&self, command: Command) -> Result<ExitCode, Box<dyn Error>> {
		assert!(!command.cmd.is_empty());

		let bwrap_args = self.get_bwrap_args(&command)?;
		let mut bwrap_command = OsCommand::new("bwrap");
		bwrap_command.args(bwrap_args);
		bwrap_command.arg("--");
		bwrap_command.args(command.cmd);

		if command.detach {
			detach_process(false)?;
		} else {
			// prevent TIOCSTI injections if controlling terminal is inherited
			seccompiler::apply_filter(&get_bpf_program()).unwrap();
		}
		let mut bwrap_process = bwrap_command.spawn().map_err(|err| {
			let mut error_string = format!("Failed to execute bwrap: {err}");
			if err.kind() == io::ErrorKind::NotFound {
				error_string.push_str(concat!(
					"\n  NOTE: This may be because Bubblewrap is not installed.",
					"\n        Install Bubblewrap (https://github.com/containers/bubblewrap)",
					"\n        and make sure `bwrap` is available in `$PATH`.",
				));
			}
			error_string
		})?;

		if command.detach {
			Ok(ExitCode::SUCCESS)
		} else {
			let sandbox_status = bwrap_process.wait().unwrap();
			Ok(convert_exit_status_to_code(sandbox_status))
		}
	}

	fn get_bwrap_args(&self, command: &Command) -> Result<Vec<OsString>, Box<dyn Error>> {
		let mut bwrap_args = Vec::new();

		match &self.envvar_whitelist {
			EnvVarWhitelist::All => (),
			EnvVarWhitelist::List(list) => {
				bwrap_args.push("--clearenv".into());
				bwrap_args.append(&mut get_envvar_whitelist_args(list));
			}
		}

		assert!(command.working_dir.is_absolute());
		bwrap_args.extend_from_slice(&["--chdir".into(), command.working_dir.clone().into()]);
		bwrap_args.extend_from_slice(&["--proc".into(), "/proc".into()]);
		//NOTE: as this argument appears before the virtual fs arguments,
		//      it is possible to whitelist subpaths of /dev
		bwrap_args.extend_from_slice(&["--dev".into(), "/dev".into()]);

		bwrap_args.append(&mut get_virtual_fs_args(&self.fs_tree)?);

		bwrap_args.extend_from_slice(&[
			"--unshare-user".into(),
			"--unshare-ipc".into(),
			"--unshare-pid".into(),
			"--unshare-cgroup-try".into(),
		]);

		// ensure that the sandbox command is terminated when the sandbox is closed
		if !command.detach {
			bwrap_args.push("--die-with-parent".into());
		}

		Ok(bwrap_args)
	}
}
fn get_virtual_fs_args(fs_tree: &VirtualFSTree<()>) -> Result<Vec<OsString>, Box<dyn Error>> {
	let mut args = Vec::new();
	for (path, ty) in fs_tree.flatten() {
		assert!(path.is_absolute());
		let mut path_args = match ty {
			VirtualFSEntryType::AllowDev => {
				vec!["--dev-bind-try".into(), path.clone().into(), path.into()]
			}
			VirtualFSEntryType::ReadWrite => {
				vec!["--bind-try".into(), path.clone().into(), path.into()]
			}
			VirtualFSEntryType::ReadOnly => {
				vec!["--ro-bind-try".into(), path.clone().into(), path.into()]
			}
			VirtualFSEntryType::Symlink => {
				let target_path = path
					.read_link()
					.map_err(|err| format!("Failed to read symlink `{}`: {err}", path.display()))?;
				//TODO: consider security implications of the change in ownership
				vec!["--symlink".into(), target_path.into(), path.into()]
			}
			VirtualFSEntryType::Tmpfs => {
				vec!["--tmpfs".into(), path.as_os_str().to_owned()]
			}
		};
		args.append(&mut path_args);
	}
	Ok(args)
}
fn get_envvar_whitelist_args(envvar_whitelists: &[OsString]) -> Vec<OsString> {
	let mut args = Vec::new();
	for envvar in envvar_whitelists {
		let Some(var_value) = env::var_os(envvar) else {
			continue;
		};
		args.extend_from_slice(&["--setenv".into(), envvar.into(), var_value]);
	}
	args
}
fn convert_exit_status_to_code(status: ExitStatus) -> ExitCode {
	if let Some(code) = status.code() {
		(code as u8).into()
	} else if status.success() {
		ExitCode::SUCCESS
	} else {
		ExitCode::FAILURE
	}
}

impl Command {
	// run command without a sandbox
	pub fn run(&self) -> Result<ExitCode, Box<dyn Error>> {
		assert!(!self.cmd.is_empty());

		if self.detach {
			detach_process(false)?;
		};

		let mut child = OsCommand::new(&self.cmd[0])
			.args(self.cmd.iter().skip(1))
			.current_dir(&self.working_dir)
			.spawn()
			.map_err(|err| format!("Failed to execute command `{}`: {err}", &self.cmd[0]))?;

		if self.detach {
			Ok(ExitCode::SUCCESS)
		} else {
			let child_status = child.wait().unwrap();
			Ok(convert_exit_status_to_code(child_status))
		}
	}
}
// detach this process from the controlling terminal and
// redirect stdout/stderr to a logfile
pub fn detach_process(keep_working_dir: bool) -> Result<(), String> {
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
	// leak the file descriptor
	let logfile_fd = logfile.into_raw_fd();

	println!(
		concat!(
			"NOTE: Detaching from terminal;\n",
			"      further output will be redirected to `{}`",
		),
		logfile_path.display()
	);
	// wrapper of dup2 handling EINTR
	let dup2 = |oldfd, newfd| loop {
		match unistd::dup2(oldfd, newfd) {
			Err(Errno::EINTR) => (),
			other => return other,
		}
	};
	dup2(logfile_fd, 1).unwrap();
	dup2(logfile_fd, 2).unwrap();
	unistd::close(0).unwrap();

	unistd::daemon(keep_working_dir, true)
		.map_err(|err| format!("Failed to detach process: {err}"))?;

	Ok(())
}
// remove logfiles older than 24h, errors are silently ignored
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
				// remove the file in case of an error
				return false;
			};
			elapsed_time > Duration::from_secs(60 * 60 * 24)
		})
		.for_each(|dir_entry| {
			// NOTE: directories are not removed
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

	Err(io::Error::new(
		io::ErrorKind::Other,
		"all logfile names are occupied",
	))
}

// path tree of all virtual-fs-entries with the following normalization:
// 1. All subpaths of a path can only have higher permissions.
//    If this is not the case, the tree is silently normalized.
// 2. Tmpfs/Symlinks cannot have any whitelists in subpaths.
//    If this is not the case, an error is returned.
#[derive(Clone)]
pub struct VirtualFSTree<U> {
	// the current component of the path
	path_component: OsString,
	children: Vec<VirtualFSTree<U>>,
	// may contain a virtual-fs-entry of the current path
	// U can be used for user data to identify paths
	// in the event of an error
	entry: Option<(VirtualFSEntryType, U)>,
}
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum VirtualFSEntryType {
	// whitelists
	AllowDev,
	ReadWrite,
	ReadOnly,
	// others (need to be leafs)
	Tmpfs,
	Symlink,
}
#[derive(Clone, Debug)]
pub enum FSTreeError<U> {
	// 'inner_path' is not allowed to have children, but invalid_child is one
	IllegalChildren { inner_path: U, invalid_child: U },
	ConflictingEntries(U, U),
}
impl<U: Clone> VirtualFSTree<U> {
	pub fn new() -> Self {
		Self {
			path_component: "/".into(),
			children: Vec::new(),
			entry: None,
		}
	}
	pub fn remove_user_data(self) -> VirtualFSTree<()> {
		VirtualFSTree {
			path_component: self.path_component,
			children: self
				.children
				.into_iter()
				.map(Self::remove_user_data)
				.collect(),
			entry: self.entry.map(|(ty, _)| (ty, ())),
		}
	}
	pub fn add_path(
		&mut self,
		path: impl AsRef<Path>,
		ty: VirtualFSEntryType,
		user_data: U,
	) -> Result<(), FSTreeError<U>> {
		let mut path_components = path.as_ref().components();

		assert_eq!(path_components.next(), Some(PathComponents::RootDir));
		let rest_components = path_components
			.map(|comp| match comp {
				PathComponents::Normal(comp) => Some(comp),
				_ => None,
			})
			.collect::<Option<Vec<_>>>()
			.expect("unexpected path component");

		self.add_path_rec(&rest_components, (ty, user_data))
	}
	fn add_path_rec(
		&mut self,
		parts: &[&std::ffi::OsStr],
		entry: (VirtualFSEntryType, U),
	) -> Result<(), FSTreeError<U>> {
		if let Some(next_part) = parts.first() {
			if self.should_be_leaf() {
				return Err(FSTreeError::IllegalChildren {
					inner_path: self.entry.as_ref().unwrap().1.clone(),
					invalid_child: entry.1,
				});
			}
			if self.entry.as_ref().is_some_and(|(ty, _)| ty >= &entry.0) {
				return Ok(());
			}

			let matching_children = if let Some(existing_children) = self
				.children
				.iter_mut()
				.find(|p| &p.path_component == next_part)
			{
				existing_children
			} else {
				self.children.push(VirtualFSTree {
					path_component: next_part.into(),
					children: Vec::new(),
					entry: None,
				});
				self.children.last_mut().unwrap()
			};
			matching_children.add_path_rec(&parts[1..], entry)
		} else if !entry.0.should_be_leaf() {
			if self.should_be_leaf() {
				return Err(FSTreeError::ConflictingEntries(
					self.entry.as_ref().unwrap().1.clone(),
					entry.1,
				));
			}
			if self.entry.as_ref().is_some_and(|(ty, _)| ty >= &entry.0) {
				return Ok(());
			}

			self.filter_subpaths(entry.0);
			self.entry = Some(entry);
			Ok(())
		} else {
			match &self.entry {
				Some((ty, _)) if ty == &entry.0 => return Ok(()),
				Some((_, u)) => return Err(FSTreeError::ConflictingEntries(u.clone(), entry.1)),
				None => (),
			}

			self.entry = Some(entry);
			let invalid_child = self.children.first().cloned();
			// clear children even in an event of an error so the tree remains valid
			self.children.clear();

			if let Some(invalid_child) = invalid_child {
				Err(FSTreeError::IllegalChildren {
					inner_path: self.entry.as_ref().unwrap().1.clone(),
					invalid_child: invalid_child.find_subpath_entry().1.clone(),
				})
			} else {
				Ok(())
			}
		}
	}
	fn should_be_leaf(&self) -> bool {
		self
			.entry
			.as_ref()
			.is_some_and(|(ty, _)| ty.should_be_leaf())
	}
	// filter out subpaths with lower permissions
	fn filter_subpaths(&mut self, ty: VirtualFSEntryType) -> bool {
		if self
			.entry
			.as_ref()
			.is_some_and(|(self_ty, _)| self_ty <= &ty)
		{
			self.entry = None;
		}

		let mut found_subpath_whitelist = false;
		self.children.retain_mut(|child| {
			let child_val = child.filter_subpaths(ty);
			found_subpath_whitelist |= child_val;
			child_val
		});

		found_subpath_whitelist |= self.entry.is_some();
		found_subpath_whitelist
	}
	fn find_subpath_entry(&self) -> &(VirtualFSEntryType, U) {
		if let Some(entry) = &self.entry {
			return entry;
		}
		assert!(!self.children.is_empty());
		self.children[0].find_subpath_entry()
	}
	fn flatten(&self) -> Vec<(PathBuf, VirtualFSEntryType)> {
		let mut entries = Vec::new();
		let path: PathBuf = self.path_component.clone().into();

		if let Some(entry) = &self.entry {
			entries.push((path.clone(), entry.0));
		}

		for child in &self.children {
			let child_entries = child
				.flatten()
				.into_iter()
				.map(|entry| (path.join(entry.0), entry.1));
			entries.extend(child_entries);
		}

		entries
	}
}
impl VirtualFSEntryType {
	fn priority(&self) -> Option<i64> {
		match self {
			VirtualFSEntryType::AllowDev => Some(2),
			VirtualFSEntryType::ReadWrite => Some(1),
			VirtualFSEntryType::ReadOnly => Some(0),
			VirtualFSEntryType::Symlink => Some(-1),
			VirtualFSEntryType::Tmpfs => None,
		}
	}
	fn should_be_leaf(&self) -> bool {
		matches!(
			self,
			VirtualFSEntryType::Tmpfs | VirtualFSEntryType::Symlink
		)
	}
}
impl PartialOrd for VirtualFSEntryType {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		let self_prio = self.priority()?;
		let other_prio = other.priority()?;
		Some(self_prio.cmp(&other_prio))
	}
}

// blacklists TIOCSTI
fn get_bpf_program() -> BpfProgram {
	#[cfg(target_arch = "x86_64")]
	let arch = SeccompArch::x86_64;
	#[cfg(target_arch = "aarch64")]
	let arch = SeccompArch::aarch64;
	#[cfg(target_arch = "riscv64")]
	let arch = SeccompArch::riscv64;
	#[cfg(not(any(
		target_arch = "aarch64",
		target_arch = "x86_64",
		target_arch = "riscv64"
	)))]
	compile_error!("only x86_64, aarch64 and riscv64 are supported");

	let blacklist_syscalls = [(
		libc::SYS_ioctl,
		vec![SeccompRule::new(vec![SeccompCondition::new(
			1,
			SeccompCmpArgLen::Dword,
			SeccompCmpOp::MaskedEq(0xFFFF_FFFF),
			libc::TIOCSTI,
		)
		.unwrap()])
		.unwrap()],
	)];
	SeccompFilter::new(
		blacklist_syscalls.into_iter().collect(),
		SeccompAction::Allow,
		SeccompAction::Trap,
		arch,
	)
	.unwrap()
	.try_into()
	.unwrap()
}
