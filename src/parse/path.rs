use std::{env, ops::RangeInclusive, path::PathBuf};

use crate::paths;

#[derive(Debug, derive_more::From, derive_more::Display)]
pub enum Error {
	#[display(fmt = "illformed")]
	Illformed,
	#[display(fmt = "expands to invalid UTF-8")]
	InvalidUtf8,
	#[display(fmt = "{}", "Self::display_env_var(name, err)")]
	EnvVar { name: String, err: env::VarError },
	#[display(fmt = "unknown variable")]
	UnkownVariable,
	#[display(fmt = "could not find this include file")]
	IncludeFileNotFound,
	#[display(fmt = "found multiple matching files: {files:?}")]
	MultipleMatchingIncludeFiles { files: Vec<PathBuf> },
	#[display(fmt = "this path has to be absolute")]
	RelativePath,

	#[allow(clippy::enum_variant_names)]
	#[from]
	PathsError(paths::Error),
}
impl Error {
	fn display_env_var(name: &str, err: &env::VarError) -> String {
		match err {
			env::VarError::NotPresent => format!("environment variable `{name}` not found"),
			env::VarError::NotUnicode(_) => {
				format!("environment variable `{name}` was not valid unicode")
			}
		}
	}
}
impl std::error::Error for Error {}
type ModResult<T> = Result<T, Error>;

pub fn canonicalize_path(path: impl Into<String>) -> ModResult<PathBuf> {
	let path = PathBuf::from(substitute_placeholder(path, false)?);
	if path.is_relative() {
		return Err(Error::RelativePath);
	}
	Ok(path)
}

//TODO: nested expressions
pub fn substitute_placeholder(str: impl Into<String>, allow_file_var: bool) -> ModResult<String> {
	let mut str = str.into();

	#[derive(Debug)]
	struct Placeholder {
		range: RangeInclusive<usize>,
		square_bracket_range: bool,
	}
	impl Placeholder {
		fn shift(self, amount: usize) -> Self {
			Self {
				range: *self.range.start() + amount..=*self.range.end() + amount,
				..self
			}
		}
	}
	fn find_next_placeholder(str: &str) -> ModResult<Option<Placeholder>> {
		let square_start_seq_idx = str.find("$[");
		let round_start_seq_idx = str.find("$(");
		let search_square_placeholder = match (square_start_seq_idx, round_start_seq_idx) {
			(None, None) => return Ok(None),
			(Some(_), None) => true,
			(None, Some(_)) => false,
			(Some(square_idx), Some(round_idx)) => square_idx < round_idx,
		};

		let start_seq_idx = if search_square_placeholder {
			square_start_seq_idx.unwrap()
		} else {
			round_start_seq_idx.unwrap()
		};
		let end_seq_idx = str[start_seq_idx..]
			.find(if search_square_placeholder { ']' } else { ')' })
			.ok_or(Error::Illformed)?
			+ start_seq_idx;

		Ok(Some(Placeholder {
			range: start_seq_idx..=end_seq_idx,
			square_bracket_range: search_square_placeholder,
		}))
	}

	// str[str_pointer..] is the still unfinished substr
	let mut str_pointer = 0;
	loop {
		let Some(next_placeholder) = find_next_placeholder(&str[str_pointer..])? else {
			break;
		};
		let next_placeholder = next_placeholder.shift(str_pointer);

		let expr = &str[next_placeholder.range.start() + 2..=next_placeholder.range.end() - 1];
		// NOTE: can be None if the expression must be resolved at a later point,
		//       e.g. $(FILE)
		let resolved_expr = if next_placeholder.square_bracket_range {
			Some(resolve_envvar_expr(expr)?)
		} else {
			resolve_variable_expr(expr, allow_file_var)?
		};

		if let Some(resolved_expr) = &resolved_expr {
			str.replace_range(next_placeholder.range, resolved_expr);
			str_pointer += resolved_expr.len();
		} else {
			str_pointer = next_placeholder.range.end() + 1;
		}
	}

	if str.contains('~') {
		Ok(str.replace(
			'~',
			paths::get_home_dir()?.to_str().ok_or(Error::InvalidUtf8)?,
		))
	} else {
		Ok(str)
	}
}
fn resolve_envvar_expr(expr: &str) -> ModResult<String> {
	let parts = expr.split(':').collect::<Vec<_>>();
	if parts.is_empty() || parts.len() > 2 {
		return Err(Error::Illformed);
	}
	let env_var_name = parts[0];
	let env_var_alt = parts.get(1).map(ToString::to_string);

	match env::var(env_var_name) {
		Ok(value) => Ok(value),
		Err(env::VarError::NotPresent) if env_var_alt.is_some() => Ok(env_var_alt.unwrap()),
		Err(err) => Err(Error::EnvVar {
			name: env_var_name.to_string(),
			err,
		}),
	}
}
fn resolve_variable_expr(expr: &str, allow_file_var: bool) -> ModResult<Option<String>> {
	let path = match expr {
		"CONFIG" => paths::get_xdg_config_dir()?,
		"CACHE" => paths::get_xdg_cache_dir()?,
		"DATA" => paths::get_xdg_data_dir()?,
		"STATE" => paths::get_xdg_state_dir()?,
		"FILE" if allow_file_var => return Ok(None),
		_ => return Err(Error::UnkownVariable),
	};

	Ok(Some(path.to_str().ok_or(Error::InvalidUtf8)?.to_string()))
}

pub fn canonicalize_include_path(path: impl Into<String>) -> ModResult<PathBuf> {
	let path = PathBuf::from(substitute_placeholder(path.into(), false)?);

	if path.is_absolute() {
		return Ok(path);
	};

	let mut possible_files = Vec::new();
	for data_root_dir in paths::get_skeld_data_dirs()? {
		let include_root_dir = data_root_dir.join("include");
		let possible_file_path = include_root_dir.join(&path);
		if possible_file_path.exists() {
			possible_files.push(possible_file_path);
		}
	}

	if possible_files.is_empty() {
		Err(Error::IncludeFileNotFound)
	} else if possible_files.len() >= 2 {
		Err(Error::MultipleMatchingIncludeFiles {
			files: possible_files,
		})
	} else {
		assert!(possible_files.len() == 1);
		Ok(possible_files.into_iter().next().unwrap())
	}
}
