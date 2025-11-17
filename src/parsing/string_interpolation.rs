use std::{env, iter, ops::Range, path::PathBuf};

use crate::{
	dirs,
	parsing::lib::{CanonicalizationError, CanonicalizationLabel},
};

pub fn resolve_placeholders(str: &str) -> Result<String, CanonicalizationError> {
	let var_resolver = StandardVariableResolver {
		unallowed_file_var_note: "$(FILE) can only be used in 'editor.cmd'",
	};
	raw_resolve_placeholders(str, &var_resolver).map_err(|err| match err {
		InternalError::Other(err) => err,
		InternalError::UnresolvableFileVar => unreachable!(),
	})
}
pub fn resolve_placeholders_in_editor_program(str: &str) -> Result<String, CanonicalizationError> {
	let var_resolver = StandardVariableResolver {
		unallowed_file_var_note: "$(FILE) cannot be used in the program path",
	};
	raw_resolve_placeholders(str, &var_resolver).map_err(|err| match err {
		InternalError::Other(err) => err,
		InternalError::UnresolvableFileVar => unreachable!(),
	})
}
pub fn resolve_placeholders_with_file(
	str: &str,
	file_value: Option<&str>,
) -> Result<Option<String>, CanonicalizationError> {
	let var_resolver = VariableResolverWithFile {
		file_var_value: file_value,
	};
	match raw_resolve_placeholders(str, &var_resolver) {
		Ok(str) => Ok(Some(str)),
		Err(InternalError::UnresolvableFileVar) => Ok(None),
		Err(InternalError::Other(err)) => Err(err),
	}
}

trait VariableResolver {
	fn resolve(&self, expr: &str) -> Result<String, InternalError>;
}
#[derive(Clone, Debug, derive_more::From)]
enum InternalError {
	UnresolvableFileVar,
	#[from]
	Other(CanonicalizationError),
}
impl InternalError {
	fn shift(self, amount: usize) -> Self {
		match self {
			Self::Other(err) => Self::Other(err.shift(amount)),
			Self::UnresolvableFileVar => self,
		}
	}
}
fn raw_resolve_placeholders(
	str: &str,
	var_resolver: &dyn VariableResolver,
) -> Result<String, InternalError> {
	let resolve_placeholder = |placeholder| match placeholder {
		Placeholder::Tilde { idx: pos } => resolve_homedir_expr(&str[pos..pos + 1])
			.map_err(|err| err.shift(pos))
			.map(|str| (pos..pos + 1, str)),
		Placeholder::BracketPair {
			ty: BracketType::Square,
			span,
			inner_span,
		} => resolve_envvar_expr(&str[inner_span.clone()], var_resolver)
			.map_err(|err| err.shift(inner_span.start))
			.map(|str| (span, str)),
		Placeholder::BracketPair {
			ty: BracketType::Round,
			span,
			inner_span,
		} => var_resolver
			.resolve(&str[inner_span.clone()])
			.map_err(|err| err.shift(inner_span.start))
			.map(|str| (span, str)),
	};
	let replacements = find_toplevel_placeholders(str)?
		.into_iter()
		.map(resolve_placeholder)
		.collect::<Result<Vec<_>, _>>()?;

	let substituted_str = replace_multiple_ranges(str, replacements);
	Ok(substituted_str)
}
fn resolve_homedir_expr(expr: &str) -> Result<String, InternalError> {
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
fn resolve_envvar_expr(
	expr: &str,
	var_resolver: &dyn VariableResolver,
) -> Result<String, InternalError> {
	let first_colon = expr.find(':');
	let env_var_name = first_colon.map(|pos| &expr[..pos]).unwrap_or(expr);
	let fallback_value = first_colon.map(|pos| &expr[pos + 1..]);

	if let Some(placeholder) = find_next_placeholder_poi(env_var_name) {
		return Err(
			CanonicalizationError {
				labels: vec![CanonicalizationLabel::primary_with_span(
					placeholder.0,
					"placeholders are not allowed here",
				)],
				..CanonicalizationError::main_message("invalid environment variable expression")
			}
			.into(),
		);
	}

	match env::var(env_var_name) {
		Ok(value) => Ok(value),
		Err(env::VarError::NotPresent) if fallback_value.is_some() => {
			let fallback_value = fallback_value.unwrap();
			raw_resolve_placeholders(fallback_value, var_resolver)
				.map_err(|err| err.shift(env_var_name.len() + 1))
		}
		Err(env::VarError::NotPresent) => Err(
			CanonicalizationError {
				labels: vec![CanonicalizationLabel::primary_with_span(
					0..env_var_name.len(),
					"",
				)],
				..CanonicalizationError::main_message("environment variable not found")
			}
			.into(),
		),
		Err(env::VarError::NotUnicode(raw)) => Err(
			CanonicalizationError {
				labels: vec![CanonicalizationLabel::primary_with_span(
					0..env_var_name.len(),
					format!("raw value: `{}`", raw.to_string_lossy()),
				)],
				..CanonicalizationError::main_message("environment variable was not valid UTF-8")
			}
			.into(),
		),
	}
}

#[derive(Clone, Debug)]
struct StandardVariableResolver<'a> {
	unallowed_file_var_note: &'a str,
}
impl VariableResolver for StandardVariableResolver<'_> {
	fn resolve(&self, expr: &str) -> Result<String, InternalError> {
		let (var_name, fallback_value) = parse_variable(expr)?;

		if let Some(resolved) = resolve_standard_variables(var_name, fallback_value)? {
			return Ok(resolved);
		}

		let mut err = make_unknown_variable_error(var_name, false);
		if var_name == "FILE" {
			err.notes.push(self.unallowed_file_var_note.to_owned());
		}
		Err(err.into())
	}
}
#[derive(Clone, Debug)]
struct VariableResolverWithFile<'a> {
	file_var_value: Option<&'a str>,
}
impl VariableResolver for VariableResolverWithFile<'_> {
	fn resolve(&self, expr: &str) -> Result<String, InternalError> {
		let (var_name, fallback_value) = parse_variable(expr)?;

		if let Some(resolved) = resolve_standard_variables(var_name, fallback_value)? {
			return Ok(resolved);
		}

		if var_name == "FILE" {
			return if let Some(file_var_value) = self.file_var_value {
				Ok(file_var_value.to_owned())
			} else if let Some(fallback_value) = fallback_value {
				raw_resolve_placeholders(fallback_value, self)
			} else {
				Err(InternalError::UnresolvableFileVar)
			};
		}

		Err(make_unknown_variable_error(var_name, true).into())
	}
}

fn parse_variable(expr: &str) -> Result<(&str, Option<&str>), CanonicalizationError> {
	let first_colon = expr.find(':');
	let var_name = first_colon.map(|pos| &expr[..pos]).unwrap_or(expr);
	let fallback_value = first_colon.map(|pos| &expr[pos + 1..]);

	if let Some(placeholder) = find_next_placeholder_poi(var_name) {
		return Err(CanonicalizationError {
			labels: vec![CanonicalizationLabel::primary_with_span(
				placeholder.0,
				"placeholders are not allowed inside variable expressions",
			)],
			..CanonicalizationError::main_message("invalid variable expression")
		});
	}

	Ok((var_name, fallback_value))
}
fn resolve_standard_variables(
	var_name: &str,
	fallback_value: Option<&str>,
) -> Result<Option<String>, CanonicalizationError> {
	type XdgDirFn = fn() -> Result<PathBuf, dirs::Error>;
	let dir_exprs = [
		("CONFIG", dirs::get_xdg_config_dir as XdgDirFn),
		("CACHE", dirs::get_xdg_cache_dir),
		("DATA", dirs::get_xdg_data_dir),
		("STATE", dirs::get_xdg_state_dir),
	];
	for (varname, resolve_fn) in dir_exprs {
		if var_name != varname {
			continue;
		}

		if let Some(fallback_value) = fallback_value {
			return Err(CanonicalizationError {
				labels: vec![CanonicalizationLabel::primary_with_span(
					// assumes the format to be $(<var name>:<fallback>)
					var_name.len()..var_name.len() + 1 + fallback_value.len(),
					format!("fallback values are not supported for $({var_name})"),
				)],
				..CanonicalizationError::main_message("invalid variable expression")
			});
		}

		let dirname = varname.to_lowercase();
		let resolved_expr_path =
			resolve_fn().map_err(|err| convert_dirs_err(err, 0..var_name.len(), Some(&dirname)))?;
		let resolved_expr_str = resolved_expr_path
			.to_str()
			.ok_or_else(|| CanonicalizationError {
				labels: vec![CanonicalizationLabel::primary_with_span(
					0..var_name.len(),
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

	Ok(None)
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
			labels: error_labels,
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
			labels: error_labels,
			notes: vec![dirs::Error::RelativeHomeDir { dir }.to_string()],
			..CanonicalizationError::main_message("invalid home directory")
		},
		dirs::Error::RelativeXdgBaseDir { .. } if xdg_dirname.is_none() => unreachable!(),
		dirs::Error::RelativeXdgBaseDir { varname, dir } => {
			let xdg_dirname = xdg_dirname.unwrap();
			CanonicalizationError {
				labels: error_labels,
				notes: vec![dirs::Error::RelativeXdgBaseDir { varname, dir }.to_string()],
				..CanonicalizationError::main_message(format!("invalid {xdg_dirname} directory"))
			}
		}
	}
}
fn make_unknown_variable_error(expr: &str, file_var_allowed: bool) -> CanonicalizationError {
	let mut valid_variables = vec!["CONFIG", "CACHE", "DATA", "STATE"];
	if file_var_allowed {
		valid_variables.push("FILE");
	}
	let valid_variables_str = valid_variables
		.into_iter()
		.map(|str| format!("`$({str})`"))
		.collect::<Vec<_>>()
		.join(", ");

	CanonicalizationError {
		labels: vec![CanonicalizationLabel::primary_with_span(
			0..expr.len(),
			"unknown variable",
		)],
		notes: vec![format!(
			"supported variables are {valid_variables_str}\n(run `{man_cmd}` to see all supported variables)",
			man_cmd = crate::error::get_manpage_cmd("String Interpolation"),
		)],
		..CanonicalizationError::main_message("unknown variable expression")
	}
}

#[derive(Clone, Debug)]
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
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum BracketType {
	Square,
	Round,
}
fn find_toplevel_placeholders(str: &str) -> Result<Vec<Placeholder>, CanonicalizationError> {
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
