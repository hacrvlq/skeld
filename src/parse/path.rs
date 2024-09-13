use std::{env, iter, ops::Range, path::PathBuf};

use crate::paths;

type ModResult<T> = Result<T, std::convert::Infallible>;

pub fn canonicalize_path(path: impl Into<String>) -> ModResult<PathBuf> {
	let path = PathBuf::from(substitute_placeholder(path, false)?);
	if path.is_relative() {
		panic!();
	}
	Ok(path)
}
pub fn canonicalize_include_path(path: impl Into<String>) -> ModResult<PathBuf> {
	let path = PathBuf::from(substitute_placeholder(path, false)?);

	if path.is_absolute() {
		return Ok(path);
	};

	let mut possible_files = Vec::new();
	for data_root_dir in paths::get_skeld_data_dirs().unwrap() {
		let include_root_dir = data_root_dir.join("include");
		let possible_file_path = include_root_dir.join(&path);
		if possible_file_path.exists() {
			possible_files.push(possible_file_path);
		}
	}

	if possible_files.is_empty() {
		panic!();
	} else if possible_files.len() >= 2 {
		panic!();
	} else {
		assert!(possible_files.len() == 1);
		Ok(possible_files.into_iter().next().unwrap())
	}
}

// resolves all placeholders except $(FILE),
// allow_file_var determines whether the $(FILE) placeholder is allowed
pub fn substitute_placeholder(str: impl Into<String>, allow_file_var: bool) -> ModResult<String> {
	let str = str.into();

	let home_dir = paths::get_home_dir().unwrap();
	let home_dir = home_dir.to_str().unwrap();
	let resolve_placeholder = |placeholder| match placeholder {
		Placeholder::Tilde { idx: pos } => (pos..pos + 1, home_dir.to_string()),
		Placeholder::BracketPair {
			ty: BracketType::Square,
			span,
			inner_span,
		} => (span, resolve_envvar_expr(&str[inner_span], allow_file_var)),
		Placeholder::BracketPair {
			ty: BracketType::Round,
			span,
			inner_span,
		} => {
			let resolved_expr = resolve_variable_expr(&str[inner_span], allow_file_var)
				// preserve variables that need to be resolved later
				.unwrap_or_else(|| str[span.clone()].to_string());
			(span, resolved_expr)
		}
	};

	let replacements = find_toplevel_placeholders(&str)
		.into_iter()
		.map(resolve_placeholder);
	let substituted_str = replace_multiple_ranges(&str, replacements);
	Ok(substituted_str)
}
fn resolve_envvar_expr(expr: &str, allow_file_var: bool) -> String {
	let parts = expr.split(':').collect::<Vec<_>>();
	assert!(!parts.is_empty());
	if parts.len() > 2 {
		panic!();
	}
	let env_var_name = parts[0];
	assert!(find_next_placeholder_poi(env_var_name).is_none());
	let env_var_alt = parts.get(1);

	match env::var(env_var_name) {
		Ok(value) => value,
		Err(env::VarError::NotPresent) if env_var_alt.is_some() => {
			let env_var_alt = *env_var_alt.unwrap();
			substitute_placeholder(env_var_alt, allow_file_var).unwrap()
		}
		Err(_) => panic!(),
	}
}
// NOTE: returns None if the variable needs to be resolved
//       at a later stage (e.g. $(FILE))
fn resolve_variable_expr(expr: &str, allow_file_var: bool) -> Option<String> {
	assert!(find_next_placeholder_poi(expr).is_none());

	let path = match expr {
		"CONFIG" => paths::get_xdg_config_dir().unwrap(),
		"CACHE" => paths::get_xdg_cache_dir().unwrap(),
		"DATA" => paths::get_xdg_data_dir().unwrap(),
		"STATE" => paths::get_xdg_state_dir().unwrap(),
		"FILE" if allow_file_var => return None,
		_ => panic!(),
	};

	Some(path.to_str().unwrap().to_string())
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
fn find_toplevel_placeholders(str: &str) -> Vec<Placeholder> {
	let mut placeholders = Vec::new();

	let mut bracket_stack = Vec::new();
	let mut str_pointer = 0;
	while let Some((rel_span, ty)) = find_next_placeholder_poi(&str[str_pointer..]) {
		let idx = str_pointer + rel_span.start;
		str_pointer += rel_span.end;
		match ty {
			PlaceholderPoI::Tilde if bracket_stack.is_empty() => {
				placeholders.push(Placeholder::Tilde { idx })
			}
			PlaceholderPoI::Tilde => (),
			PlaceholderPoI::Bracket { ty, opening: true } => bracket_stack.push((idx, ty)),
			PlaceholderPoI::Bracket { ty, opening: false } => {
				let matching_opening_bracket = bracket_stack.pop().unwrap();
				assert!(matching_opening_bracket.1 == ty);
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
	assert!(bracket_stack.is_empty());

	placeholders
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
