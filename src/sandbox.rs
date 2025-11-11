use std::{
	cmp::Ordering,
	env,
	error::Error,
	ffi::{OsStr, OsString},
	io::{self, Write as _},
	os::fd::IntoRawFd as _,
	path::{Component as PathComponents, Path, PathBuf},
	process::{Command as OsCommand, ExitCode},
};

use nix::unistd;
use seccompiler::{
	BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
	SeccompRule, TargetArch as SeccompArch,
};

use crate::command::{self, Command};

#[derive(Clone, Debug)]
pub struct SandboxParameters {
	pub fs_tree: VirtualFSTree<()>,
	pub envvar_whitelist: EnvVarWhitelist,
}
#[derive(Clone, Debug)]
pub enum EnvVarWhitelist {
	All,
	List(Vec<OsString>),
}
impl SandboxParameters {
	pub fn run_cmd(&self, command: Command) -> Result<ExitCode, Box<dyn Error>> {
		let mut bwrap_command = OsCommand::new("bwrap");

		let args_data = self.get_bwrap_args(&command)?.join(OsStr::new("\0"));
		let (pipe_reader, pipe_writer) = unistd::pipe().unwrap();
		io::PipeWriter::from(pipe_writer)
			.write_all(args_data.as_encoded_bytes())
			.unwrap();
		bwrap_command.args(["--args", &pipe_reader.into_raw_fd().to_string()]);

		bwrap_command.arg("--");
		bwrap_command.arg(command.program);
		bwrap_command.args(command.args);

		if command.detach {
			command::detach_from_tty()?;
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
			Ok(command::forward_child_exit_status(sandbox_status))
		}
	}

	fn get_bwrap_args(&self, command: &Command) -> Result<Vec<OsString>, Box<dyn Error>> {
		let mut bwrap_args: Vec<OsString> = Vec::new();

		if !command.detach {
			let bpf_program = get_bpf_program();

			let (pipe_reader, pipe_writer) = unistd::pipe().unwrap();
			let mut pipe_writer: io::PipeWriter = pipe_writer.into();
			let bpf_program_fd = pipe_reader.into_raw_fd();

			for instr in bpf_program {
				let instr_bytes =
					unsafe { std::slice::from_raw_parts((&raw const instr).cast(), size_of_val(&instr)) };
				pipe_writer.write_all(instr_bytes).unwrap();
			}
			drop(pipe_writer);

			bwrap_args.extend_from_slice(&["--seccomp".into(), bpf_program_fd.to_string().into()]);
		}

		match &self.envvar_whitelist {
			EnvVarWhitelist::All => (),
			EnvVarWhitelist::List(list) => {
				bwrap_args.push("--clearenv".into());
				bwrap_args.extend(get_envvar_whitelist_args(list));
			}
		}

		if let Some(working_dir) = &command.working_dir {
			assert!(working_dir.is_absolute());
			bwrap_args.extend_from_slice(&["--chdir".into(), working_dir.clone().into()]);
		}
		bwrap_args.extend_from_slice(&["--proc".into(), "/proc".into()]);
		//NOTE: as this argument appears before the virtual fs arguments,
		//      it is possible to whitelist subpaths of /dev
		bwrap_args.extend_from_slice(&["--dev".into(), "/dev".into()]);

		bwrap_args.extend(get_virtual_fs_args(&self.fs_tree)?);

		bwrap_args.extend_from_slice(&["--unshare-ipc".into(), "--unshare-pid".into()]);

		Ok(bwrap_args)
	}
}
fn get_virtual_fs_args(fs_tree: &VirtualFSTree<()>) -> Result<Vec<OsString>, Box<dyn Error>> {
	get_virtual_fs_args_rec(fs_tree, PathBuf::new())
}
fn get_virtual_fs_args_rec(
	fs_tree: &VirtualFSTree<()>,
	parent_path_prefix: PathBuf,
) -> Result<Vec<OsString>, Box<dyn Error>> {
	let path_prefix = parent_path_prefix.join(&fs_tree.path_component);

	let mut args = Vec::new();

	if let Some((entry, _)) = fs_tree.entry {
		let path_str: OsString = path_prefix.to_owned().into();
		let entry_args: &[_] = match entry {
			VirtualFSEntryType::AllowDev => &["--dev-bind-try".into(), path_str.clone(), path_str],
			VirtualFSEntryType::ReadWrite => &["--bind-try".into(), path_str.clone(), path_str],
			VirtualFSEntryType::ReadOnly => &["--ro-bind-try".into(), path_str.clone(), path_str],
			VirtualFSEntryType::Symlink => {
				let target_path = path_prefix
					.read_link()
					.map_err(|err| format!("Failed to read symlink `{}`: {err}", path_prefix.display()))?;
				&["--symlink".into(), target_path.into(), path_str]
			}
			VirtualFSEntryType::Tmpfs => &["--tmpfs".into(), path_str],
		};
		args.extend_from_slice(entry_args);
	}

	for subtree in &fs_tree.children {
		args.extend(get_virtual_fs_args_rec(subtree, path_prefix.clone())?);
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

// path tree of all virtual-fs-entries with the following normalization:
// 1. All subpaths of a path can only have higher permissions.
//    If this is not the case, the tree is silently normalized.
// 2. Tmpfs/Symlinks cannot have any whitelists in subpaths.
//    If this is not the case, an error is returned.
#[derive(Clone, Debug)]
pub struct VirtualFSTree<U> {
	// the current component of the path
	path_component: OsString,
	children: Vec<VirtualFSTree<U>>,
	// may contain a virtual-fs-entry of the current path
	// U can be used for user data to identify paths
	// in the event of an error
	entry: Option<(VirtualFSEntryType, U)>,
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
		parts: &[&OsStr],
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
}
impl VirtualFSEntryType {
	fn priority(self) -> Option<i64> {
		match self {
			VirtualFSEntryType::AllowDev => Some(2),
			VirtualFSEntryType::ReadWrite => Some(1),
			VirtualFSEntryType::ReadOnly => Some(0),
			VirtualFSEntryType::Symlink => Some(-1),
			VirtualFSEntryType::Tmpfs => None,
		}
	}
	fn should_be_leaf(self) -> bool {
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

// blacklists the TIOCSTI and TIOCLINUX ioctls
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

	const IOCTL_OP_MASK: u64 = 0xFFFF_FFFF;
	const _: () = assert!(libc::TIOCSTI & !IOCTL_OP_MASK == 0);
	const _: () = assert!(libc::TIOCLINUX & !IOCTL_OP_MASK == 0);
	let blacklist_syscalls = [(
		libc::SYS_ioctl,
		vec![
			SeccompRule::new(vec![
				SeccompCondition::new(
					1,
					SeccompCmpArgLen::Qword,
					SeccompCmpOp::MaskedEq(IOCTL_OP_MASK),
					libc::TIOCSTI,
				)
				.unwrap(),
			])
			.unwrap(),
			SeccompRule::new(vec![
				SeccompCondition::new(
					1,
					SeccompCmpArgLen::Qword,
					SeccompCmpOp::MaskedEq(IOCTL_OP_MASK),
					libc::TIOCLINUX,
				)
				.unwrap(),
			])
			.unwrap(),
		],
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
