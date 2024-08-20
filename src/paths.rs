use std::{
	env,
	ffi::{CStr, OsStr},
	os::unix::ffi::OsStrExt as _,
	path::{Path, PathBuf},
};

#[allow(clippy::enum_variant_names)]
#[derive(Debug, derive_more::Display)]
pub enum Error {
	#[display("home directory could not be determined")]
	UnknownHomeDir,
	#[display("home directory has to be absolute")]
	RelativeHomeDir,
	#[display("{name} must be absolute")]
	RelativeXdgBaseDir { name: String },
}
impl std::error::Error for Error {}
type ModResult<T> = Result<T, Error>;

pub fn get_xdg_config_dir() -> ModResult<PathBuf> {
	get_xdg_base_dir("XDG_CONFIG_HOME", ".config")
}
pub fn get_xdg_cache_dir() -> ModResult<PathBuf> {
	get_xdg_base_dir("XDG_CACHE_HOME", ".cache")
}
pub fn get_xdg_data_dir() -> ModResult<PathBuf> {
	get_xdg_base_dir("XDG_DATA_HOME", ".local/share")
}
pub fn get_xdg_state_dir() -> ModResult<PathBuf> {
	get_xdg_base_dir("XDG_STATE_HOME", ".local/state")
}
fn get_xdg_base_dir(env_var: &str, fallback: &str) -> ModResult<PathBuf> {
	match env::var_os(env_var) {
		Some(env_var_val) if !env_var_val.is_empty() => {
			let path: PathBuf = env_var_val.into();
			if path.is_relative() {
				Err(Error::RelativeXdgBaseDir {
					name: env_var.to_string(),
				})
			} else {
				Ok(path)
			}
		}
		_ => Ok(get_home_dir()?.join(fallback)),
	}
}

pub fn get_skeld_config_dir() -> ModResult<PathBuf> {
	Ok(get_xdg_config_dir()?.join("skeld"))
}
pub fn get_skeld_data_dir() -> ModResult<PathBuf> {
	Ok(get_xdg_data_dir()?.join("skeld"))
}
pub fn get_skeld_data_dirs() -> ModResult<Vec<PathBuf>> {
	Ok(vec![get_skeld_config_dir()?, get_skeld_data_dir()?])
}

pub fn get_home_dir() -> ModResult<PathBuf> {
	let home_dir_path = if let Some(home_dir) = env::var_os("HOME") {
		home_dir.into()
	} else {
		let passwd_ptr = unsafe { libc::getpwuid(libc::getuid()) };
		if passwd_ptr.is_null() {
			return Err(Error::UnknownHomeDir);
		}
		let home_dir = unsafe { *passwd_ptr }.pw_dir;
		if home_dir.is_null() {
			return Err(Error::UnknownHomeDir);
		}
		let home_dir_bytes = unsafe { CStr::from_ptr(home_dir) }.to_bytes();
		Path::new(OsStr::from_bytes(home_dir_bytes)).to_path_buf()
	};

	if !home_dir_path.is_absolute() {
		return Err(Error::RelativeHomeDir);
	}

	Ok(home_dir_path)
}
