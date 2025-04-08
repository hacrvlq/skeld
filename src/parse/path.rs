use std::{env, iter, ops::Range, path::PathBuf};

use crate::{
	dirs,
	parse::lib::{CanonicalizationError, CanonicalizationLabel},
};

type ModResult<T> = Result<T, CanonicalizationError>;

pub fn canonicalize_path(path: impl Into<String>) -> ModResult<PathBuf> {
	let path = path.into();

	let substituted_path_str = substitute_placeholder(&path, false)?;
	let substituted_path = PathBuf::from(&substituted_path_str);

	if substituted_path.is_relative() {
		let mut notes = Vec::new();
		if find_next_placeholder_poi(&path).is_some() {
			notes.push(format!(
				"after the placeholders have been resolved: `{substituted_path_str}`"
			));
		}
		return Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_without_span(
				"this path must be absolute",
			)],
			notes,
			..CanonicalizationError::main_message("unallowed relative path")
		});
	}

	Ok(substituted_path)
}
pub fn canonicalize_include_path(path: impl Into<String>) -> ModResult<PathBuf> {
	let path = PathBuf::from(substitute_placeholder(path, false)?);

	if path.is_absolute() {
		return Ok(path);
	};

	let mut matching_files = Vec::new();
	let skeld_data_dirs = dirs::get_skeld_data_dirs().map_err(|err| CanonicalizationError {
		notes: vec![err.to_string()],
		..CanonicalizationError::main_message("could not determine the skeld data directories")
	})?;
	for data_root_dir in skeld_data_dirs {
		let include_root_dir = data_root_dir.join("include");
		let mut possible_file_path = include_root_dir.join(&path);
		possible_file_path.as_mut_os_string().push(".toml");
		if possible_file_path.exists() {
			matching_files.push(possible_file_path);
		}
	}

	if matching_files.is_empty() {
		let mut notes = vec![format!(
			"include files are searched in `<SKELD-DATA>/include`\n(see `{man_cmd}` for more information)",
			man_cmd = crate::error::get_manpage_cmd("FILES"),
		)];
		if path.extension().is_some_and(|ext| ext == "toml") {
			notes.push(format!(
				"Note that an extra `toml` extension is appended, so the file `{}.toml` is actually searched.",
				path.display()
			));
		}
		Err(CanonicalizationError {
			notes,
			..CanonicalizationError::main_message("include file not found")
		})
	} else if matching_files.len() > 1 {
		let matching_files_str = matching_files
			.iter()
			.map(|path| format!("- {}", path.display()))
			.collect::<Vec<_>>()
			.join("\n");
		Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_without_span(
				"found multiple matching files",
			)],
			notes: vec![format!("matching files are:\n{matching_files_str}")],
			..CanonicalizationError::main_message("ambiguous include file")
		})
	} else {
		assert!(matching_files.len() == 1);
		Ok(matching_files.into_iter().next().unwrap())
	}
}

// resolves all placeholders except $(FILE),
// allow_file_var determines whether the $(FILE) placeholder is allowed
pub fn substitute_placeholder(str: impl Into<String>, allow_file_var: bool) -> ModResult<String> {
	let str = str.into();

	let resolve_placeholder = |placeholder| {
		Ok(match placeholder {
			Placeholder::Tilde { idx: pos } => {
				let resolved_expr =
					resolve_homedir_expr(&str[pos..pos + 1]).map_err(|err| err.shift(pos))?;
				(pos..pos + 1, resolved_expr)
			}
			Placeholder::BracketPair {
				ty: BracketType::Square,
				span,
				inner_span,
			} => {
				let resolved_expr = resolve_envvar_expr(&str[inner_span.clone()], allow_file_var)
					.map_err(|err| err.shift(inner_span.start))?;
				(span, resolved_expr)
			}
			Placeholder::BracketPair {
				ty: BracketType::Round,
				span,
				inner_span,
			} => {
				let resolved_expr = resolve_variable_expr(&str[inner_span.clone()], allow_file_var)
					.map_err(|err| err.shift(inner_span.start))?
					// preserve variables that need to be resolved later
					.unwrap_or_else(|| str[span.clone()].to_string());
				(span, resolved_expr)
			}
		})
	};

	let replacements = find_toplevel_placeholders(&str)?
		.into_iter()
		.map(resolve_placeholder)
		.collect::<ModResult<Vec<_>>>()?;
	let substituted_str = replace_multiple_ranges(&str, replacements);
	Ok(substituted_str)
}
fn resolve_homedir_expr(expr: &str) -> ModResult<String> {
	let home_dir_path =
		dirs::get_home_dir().map_err(|err| convert_dirs_err(err, 0..expr.len(), None))?;
	let home_dir_str = home_dir_path
		.to_str()
		.ok_or_else(|| CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				0..expr.len(),
				"required from here",
			)],
			notes: vec![format!(
				"home directory path contains invalid UTF-8: `{}`",
				home_dir_path.display()
			)],
			..CanonicalizationError::main_message("invalid home directory path")
		})?
		.to_string();
	Ok(home_dir_str)
}
fn resolve_envvar_expr(expr: &str, allow_file_var: bool) -> ModResult<String> {
	let first_colon = expr.find(':');
	let env_var_name = first_colon.map(|pos| &expr[..pos]).unwrap_or(expr);
	let env_var_alt = first_colon.map(|pos| &expr[pos + 1..]);

	if let Some(placeholder) = find_next_placeholder_poi(env_var_name) {
		return Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				placeholder.0,
				"placeholders are not allowed here",
			)],
			..CanonicalizationError::main_message("invalid environment variable expression")
		});
	}

	match env::var(env_var_name) {
		Ok(value) => Ok(value),
		Err(env::VarError::NotPresent) if env_var_alt.is_some() => {
			let env_var_alt = env_var_alt.unwrap();
			substitute_placeholder(env_var_alt, allow_file_var)
				.map_err(|err| err.shift(env_var_name.len() + 1))
		}
		Err(env::VarError::NotPresent) => Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				0..env_var_name.len(),
				"",
			)],
			..CanonicalizationError::main_message("environment variable not found")
		}),
		Err(env::VarError::NotUnicode(raw)) => Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				0..env_var_name.len(),
				format!("raw value: `{}`", raw.to_string_lossy()),
			)],
			..CanonicalizationError::main_message("environment variable was not valid UTF-8")
		}),
	}
}
// NOTE: returns None if the variable needs to be resolved
//       at a later stage (e.g. $(FILE))
fn resolve_variable_expr(expr: &str, allow_file_var: bool) -> ModResult<Option<String>> {
	if let Some(placeholder) = find_next_placeholder_poi(expr) {
		return Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				placeholder.0,
				"placeholders are not allowed inside variable expressions",
			)],
			..CanonicalizationError::main_message("invalid variable expression")
		});
	}

	type XdgDirFn = fn() -> Result<PathBuf, dirs::Error>;
	let dir_exprs = [
		("CONFIG", dirs::get_xdg_config_dir as XdgDirFn),
		("CACHE", dirs::get_xdg_cache_dir),
		("DATA", dirs::get_xdg_data_dir),
		("STATE", dirs::get_xdg_state_dir),
	];
	for (varname, resolve_fn) in dir_exprs {
		if expr != varname {
			continue;
		}

		let dirname = varname.to_lowercase();
		let resolved_expr_path =
			resolve_fn().map_err(|err| convert_dirs_err(err, 0..expr.len(), Some(&dirname)))?;
		let resolved_expr_str = resolved_expr_path
			.to_str()
			.ok_or_else(|| CanonicalizationError {
				labels: vec![CanonicalizationLabel::primary_with_span(
					0..expr.len(),
					"required from here",
				)],
				notes: vec![format!(
					"the {dirname} directory path contains invalid UTF-8: `{}`",
					resolved_expr_path.display(),
				)],
				..CanonicalizationError::main_message(format!("invalid {dirname} directory path"))
			})?;
		return Ok(Some(resolved_expr_str.to_string()));
	}

	if allow_file_var && expr == "FILE" {
		return Ok(None);
	}

	// unknown variable
	{
		let mut valid_variables = vec!["CONFIG", "CACHE", "DATA", "STATE"];
		if allow_file_var {
			valid_variables.push("FILE");
		}
		let valid_variables_str = valid_variables
			.into_iter()
			.map(|str| format!("`$({str})`"))
			.collect::<Vec<_>>()
			.join(", ");

		let mut notes = vec![format!(
			"supported variables are {valid_variables_str}\n(run `{man_cmd}` to see all supported variables)",
			man_cmd = crate::error::get_manpage_cmd("String Interpolation"),
		)];
		if !allow_file_var && expr == "FILE" {
			notes.push("$(FILE) can only be used in 'editor.cmd-with-file'".to_string());
		}

		Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				0..expr.len(),
				"unknown variable",
			)],
			notes,
			..CanonicalizationError::main_message("unknown variable expression")
		})
	}
}
fn convert_dirs_err(
	err: dirs::Error,
	span: Range<usize>,
	xdg_dirname: Option<&str>,
) -> CanonicalizationError {
	let error_labels = vec![CanonicalizationLabel::primary_with_span(
		span,
		"required from here",
	)];

	match err {
		dirs::Error::UnknownHomeDir => CanonicalizationError {
			labels: error_labels.clone(),
			notes: vec![
				concat!(
					"The home directory is first looked up in `$HOME`,\n",
					"and then in `/etc/passwd` if `$HOME` does not exist."
				)
				.to_string(),
			],
			..CanonicalizationError::main_message("could not determine the home directory")
		},
		dirs::Error::RelativeHomeDir { dir } => CanonicalizationError {
			labels: error_labels.clone(),
			notes: vec![dirs::Error::RelativeHomeDir { dir }.to_string()],
			..CanonicalizationError::main_message("invalid home directory")
		},
		dirs::Error::RelativeXdgBaseDir { .. } if xdg_dirname.is_none() => unreachable!(),
		dirs::Error::RelativeXdgBaseDir { varname, dir } => {
			let xdg_dirname = xdg_dirname.unwrap();
			CanonicalizationError {
				labels: error_labels.clone(),
				notes: vec![dirs::Error::RelativeXdgBaseDir { varname, dir }.to_string()],
				..CanonicalizationError::main_message(format!("invalid {xdg_dirname} directory"))
			}
		}
	}
}

#[derive(Clone)]
enum Placeholder {
	Tilde {
		idx: usize,
	},
	BracketPair {
		ty: BracketType,
		span: Range<usize>,
		inner_span: Range<usize>,
	},
}
#[derive(Copy, Clone, PartialEq, Eq)]
enum BracketType {
	Square,
	Round,
}
fn find_toplevel_placeholders(str: &str) -> ModResult<Vec<Placeholder>> {
	let mut placeholders = Vec::new();

	let mut bracket_stack = Vec::new();
	let mut str_pointer = 0;
	while let Some((rel_span, ty)) = find_next_placeholder_poi(&str[str_pointer..]) {
		let idx = str_pointer + rel_span.start;
		str_pointer += rel_span.end;
		match ty {
			PlaceholderPoI::Tilde if bracket_stack.is_empty() => {
				placeholders.push(Placeholder::Tilde { idx });
			}
			PlaceholderPoI::Tilde => (),
			PlaceholderPoI::Bracket { ty, opening: true } => bracket_stack.push((idx, ty)),
			PlaceholderPoI::Bracket { ty, opening: false } => {
				let Some(matching_opening_bracket) = bracket_stack.pop() else {
					return Err(CanonicalizationError {
						labels: vec![CanonicalizationLabel::primary_with_span(
							idx..idx + 1,
							"unmatched closing bracket",
						)],
						..CanonicalizationError::main_message("mismatched brackets")
					});
				};
				if matching_opening_bracket.1 != ty {
					return Err(CanonicalizationError {
						labels: vec![
							CanonicalizationLabel::primary_with_span(idx..idx + 1, "wrong closing bracket type"),
							CanonicalizationLabel::secondary_with_span(
								matching_opening_bracket.0..matching_opening_bracket.0 + 2,
								"supposed opening bracket",
							),
						],
						..CanonicalizationError::main_message("mismatched brackets")
					});
				}

				if bracket_stack.is_empty() {
					placeholders.push(Placeholder::BracketPair {
						ty,
						span: matching_opening_bracket.0..idx + 1,
						inner_span: matching_opening_bracket.0 + 2..idx,
					});
				}
			}
		}
	}
	if !bracket_stack.is_empty() {
		let last_unclosed_bracket = bracket_stack.last().unwrap();
		return Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				last_unclosed_bracket.0..last_unclosed_bracket.0 + 2,
				"unmatched opening bracket",
			)],
			..CanonicalizationError::main_message("mismatched brackets")
		});
	}

	Ok(placeholders)
}
enum PlaceholderPoI {
	Bracket { ty: BracketType, opening: bool },
	Tilde,
}
fn find_next_placeholder_poi(str: &str) -> Option<(Range<usize>, PlaceholderPoI)> {
	#[rustfmt::skip]
	let poi_types = [
		("$[", PlaceholderPoI::Bracket {ty: BracketType::Square, opening: true }),
		("$(", PlaceholderPoI::Bracket {ty: BracketType::Round, opening: true }),
		("]", PlaceholderPoI::Bracket {ty: BracketType::Square, opening: false }),
		(")", PlaceholderPoI::Bracket {ty: BracketType::Round, opening: false }),
		("~", PlaceholderPoI::Tilde),
	];
	poi_types
		.into_iter()
		.filter_map(|(poi_str, ty)| {
			let poi_start_pos = str.find(poi_str)?;
			Some((poi_start_pos..poi_start_pos + poi_str.len(), ty))
		})
		.min_by_key(|(span, _)| span.start)
}

fn replace_multiple_ranges(
	str: &str,
	replacements: impl IntoIterator<Item = (Range<usize>, String)>,
) -> String {
	let replacements = replacements
		.into_iter()
		.chain(iter::once((str.len()..str.len(), String::new())));

	let mut result_string = String::new();
	let mut str_idx = 0;
	for replacement in replacements {
		assert!(str_idx <= replacement.0.start);
		result_string.push_str(&str[str_idx..replacement.0.start]);
		result_string.push_str(&replacement.1);
		str_idx = replacement.0.end;
	}

	result_string
}
