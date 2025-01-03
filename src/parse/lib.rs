use std::{
	borrow::Cow,
	cmp::PartialEq,
	fs,
	ops::Range,
	path::{Path, PathBuf},
	rc::Rc,
};

use codespan_reporting::{
	diagnostic::{self, Label as DiagLabel, LabelStyle as DiagLabelStyle},
	files as codespan_files,
};
use toml_span::Span;

use super::ModResult;
use crate::DOCS_URL;

pub use codespan_reporting::diagnostic::Severity;
pub use toml_span::value::ValueInner as TomlInnerValue;
pub type Diagnostic = diagnostic::Diagnostic<usize>;
pub type FileDatabase = codespan_files::SimpleFiles<String, String>;

// trait implementors should save current config value
// and update it with each 'try_eat' when appropiate
pub trait ConfigOption {
	// should return true if key is consumed and false otherwise
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool>;
}

// =================================================================================================
// Wrappers around 'toml_span' adding file location info
// =================================================================================================
pub fn parse_toml_file<'v>(
	path: impl AsRef<Path>,
	file_database: &mut FileDatabase,
	// file contents and root toml value need to outlive the return value
	outlivers: &'v mut (Option<String>, Option<toml_span::Value<'v>>),
) -> ModResult<TomlTable<'v>> {
	let path = path.as_ref();
	assert!(path.is_absolute());

	outlivers.0 = Some(
		fs::read_to_string(path)
			.map_err(|err| format!("Failed to read file `{}`: {err}", path.display()))?,
	);
	let file_contents = outlivers.0.as_ref().unwrap();

	let file_id =
		FileId(file_database.add(path.to_string_lossy().to_string(), file_contents.clone()));

	outlivers.1 = Some(toml_span::parse(file_contents).map_err(|err| err.to_diagnostic(file_id.0))?);
	let parsed_contents = outlivers.1.as_ref().unwrap();
	let table = parsed_contents.as_table().unwrap();

	Ok(TomlTable {
		table,
		loc: Location {
			span: parsed_contents.span,
			file: file_id,
		},
	})
}
#[derive(Clone)]
pub struct TomlKey<'a> {
	name: Cow<'a, str>,
	loc: Location,
}
impl<'a> TomlKey<'a> {
	fn from_key(key: &toml_span::value::Key<'a>, file: FileId) -> Self {
		Self {
			name: key.name.clone(),
			loc: Location {
				file,
				span: key.span,
			},
		}
	}
	pub fn name(&self) -> &str {
		self.name.as_ref()
	}
	pub fn loc(&self) -> &Location {
		&self.loc
	}
}
pub struct TomlValue<'a> {
	value: &'a TomlInnerValue<'a>,
	loc: Location,
}
impl<'a> TomlValue<'a> {
	fn from_value(value: &'a toml_span::Value<'a>, file: FileId) -> Self {
		Self {
			value: value.as_ref(),
			loc: Location {
				file,
				span: value.span,
			},
		}
	}
	pub fn loc(&self) -> &Location {
		&self.loc
	}
	pub fn as_bool(&self) -> ModResult<bool> {
		self.value.as_bool().ok_or_else(|| {
			diagnostics::wrong_type(self, &[TomlInnerValue::Boolean(Default::default())]).into()
		})
	}
	pub fn as_int(&self) -> ModResult<i64> {
		self.value.as_integer().ok_or_else(|| {
			diagnostics::wrong_type(self, &[TomlInnerValue::Integer(Default::default())]).into()
		})
	}
	pub fn as_str(&self) -> ModResult<&str> {
		self.value.as_str().ok_or_else(|| {
			diagnostics::wrong_type(self, &[TomlInnerValue::String(Default::default())]).into()
		})
	}
	pub fn as_array(&self) -> ModResult<Vec<TomlValue<'_>>> {
		Ok(
			self
				.value
				.as_array()
				.ok_or_else(|| diagnostics::wrong_type(self, &[TomlInnerValue::Array(Default::default())]))?
				.iter()
				.map(|value| TomlValue::from_value(value, self.loc().file))
				.collect(),
		)
	}
	pub fn as_table(&self) -> ModResult<TomlTable<'_>> {
		let table = self
			.value
			.as_table()
			.ok_or_else(|| diagnostics::wrong_type(self, &[TomlInnerValue::Table(Default::default())]))?;
		Ok(TomlTable {
			table,
			loc: self.loc().clone(),
		})
	}
}
pub struct TomlTable<'a> {
	table: &'a toml_span::value::Table<'a>,
	loc: Location,
}
impl<'a> TomlTable<'a> {
	pub fn iter(&self) -> impl Iterator<Item = (TomlKey<'a>, TomlValue<'a>)> {
		let file_id = self.loc().file;
		self.table.iter().map(move |(key, value)| {
			(
				TomlKey::from_key(key, file_id),
				TomlValue::from_value(value, file_id),
			)
		})
	}
	pub fn loc(&self) -> &Location {
		&self.loc
	}
}

#[derive(Clone)]
pub struct Location {
	pub file: FileId,
	pub span: Span,
}
impl Location {
	pub fn get_primary_label(&self) -> DiagLabel<usize> {
		DiagLabel::primary(self.file.0, self.span)
	}
	pub fn get_secondary_label(&self) -> DiagLabel<usize> {
		DiagLabel::secondary(self.file.0, self.span)
	}
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FileId(usize);

// ====================================================================================================
// Basic config options
// ====================================================================================================
// helper config option that eats only a specific key using a specified parse function
#[derive(Clone)]
pub struct BaseOption<T> {
	#[allow(clippy::type_complexity)]
	parse_fn: Rc<dyn Fn(&TomlValue) -> ModResult<T>>,
	name: String,
	value: Option<(T, Location)>,
}
impl<T> BaseOption<T> {
	pub fn new(name: &str, parse_fn: impl Fn(&TomlValue) -> ModResult<T> + 'static) -> Self {
		Self {
			parse_fn: Rc::new(parse_fn),
			name: name.to_string(),
			value: None,
		}
	}
	pub fn get_value(self) -> Option<T> {
		self.value.map(|(value, _)| value)
	}
}
impl<T: PartialEq> ConfigOption for BaseOption<T> {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		if key.name != self.name {
			return Ok(false);
		}

		let value = (self.parse_fn)(value)?;
		match &self.value {
			Some(prev_val) if prev_val.0 != value => {
				return Err(diagnostics::multiple_definitions(&prev_val.1, key.loc(), &self.name).into());
			}
			_ => (),
		}
		self.value = Some((value, key.loc().clone()));
		Ok(true)
	}
}

#[derive(Clone)]
pub struct BoolOption(BaseOption<bool>);
impl BoolOption {
	pub fn new(name: &str) -> Self {
		Self(BaseOption::new(name, |value| value.as_bool()))
	}
	pub fn get_value(self) -> Option<bool> {
		self.0.get_value()
	}
}
impl ConfigOption for BoolOption {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		self.0.try_eat(key, value)
	}
}

#[derive(Clone)]
pub struct PathBufOption(BaseOption<PathBuf>);
impl PathBufOption {
	pub fn new(
		name: &str,
		canonicalization: impl Fn(&str) -> Result<PathBuf, CanonicalizationError> + 'static,
	) -> Self {
		Self(BaseOption::new(name, move |value| {
			let raw_value = value.as_str()?;
			(canonicalization)(raw_value)
				.map_err(|err| diagnostics::failed_canonicalization(value, &err).into())
		}))
	}
	pub fn get_value(self) -> Option<PathBuf> {
		self.0.get_value()
	}
}
impl ConfigOption for PathBufOption {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		self.0.try_eat(key, value)
	}
}
#[derive(Clone)]
pub struct StringOption(BaseOption<String>);
impl StringOption {
	pub fn new(name: &str) -> Self {
		Self::new_with_canonicalization(name, |str| Ok(str.to_string()))
	}
	pub fn new_with_canonicalization(
		name: &str,
		canonicalization: impl Fn(&str) -> Result<String, CanonicalizationError> + 'static,
	) -> Self {
		Self(BaseOption::new(name, move |value| {
			let raw_value = value.as_str()?;
			(canonicalization)(raw_value)
				.map_err(|err| diagnostics::failed_canonicalization(value, &err).into())
		}))
	}
	pub fn get_value(self) -> Option<String> {
		self.0.get_value()
	}
}
impl ConfigOption for StringOption {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		self.0.try_eat(key, value)
	}
}

#[derive(Clone)]
pub struct CanonicalizationError {
	pub main_message: String,
	pub labels: Vec<CanonicalizationLabel>,
	pub notes: Vec<String>,
}
#[derive(Clone)]
pub struct CanonicalizationLabel {
	pub ty: DiagLabelStyle,
	pub span: Option<Range<usize>>,
	pub message: String,
}
impl CanonicalizationError {
	pub fn main_message(msg: impl Into<String>) -> Self {
		Self {
			main_message: msg.into(),
			labels: vec![CanonicalizationLabel::primary_without_span("")],
			notes: Vec::new(),
		}
	}
	pub fn shift(self, amount: usize) -> Self {
		Self {
			labels: self
				.labels
				.into_iter()
				.map(|label| label.shift(amount))
				.collect(),
			..self
		}
	}
}
impl CanonicalizationLabel {
	pub fn primary_with_span(span: Range<usize>, msg: impl Into<String>) -> Self {
		Self {
			ty: DiagLabelStyle::Primary,
			span: Some(span),
			message: msg.into(),
		}
	}
	pub fn primary_without_span(msg: impl Into<String>) -> Self {
		Self {
			ty: DiagLabelStyle::Primary,
			span: None,
			message: msg.into(),
		}
	}
	pub fn secondary_with_span(span: Range<usize>, msg: impl Into<String>) -> Self {
		Self {
			ty: DiagLabelStyle::Secondary,
			span: Some(span),
			message: msg.into(),
		}
	}
	#[expect(unused)]
	pub fn secondary_without_span(msg: impl Into<String>) -> Self {
		Self {
			ty: DiagLabelStyle::Secondary,
			span: None,
			message: msg.into(),
		}
	}

	pub fn shift(self, amount: usize) -> Self {
		Self {
			span: self.span.map(|span| span.start + amount..span.end + amount),
			..self
		}
	}
}

#[derive(Clone)]
pub struct ArrayOption<V> {
	name: String,
	// value: Option<(_, key location, value location)>
	value: Option<(Vec<V>, Location, Location)>,
	parse_entry_fn: fn(&TomlValue) -> ModResult<V>,
	mergable: bool,
}
impl<V> ArrayOption<V> {
	pub fn new(name: &str, mergable: bool, parse_entry_fn: fn(&TomlValue) -> ModResult<V>) -> Self {
		Self {
			name: name.to_string(),
			value: None,
			parse_entry_fn,
			mergable,
		}
	}
	pub fn get_value(self) -> Option<Vec<V>> {
		self.value.map(|(v, _, _)| v)
	}
	pub fn get_value_with_loc(self) -> Option<(Vec<V>, Location)> {
		self.value.map(|(v, _, loc)| (v, loc))
	}
}
impl<V> ConfigOption for ArrayOption<V> {
	fn try_eat(&mut self, key: &TomlKey, value: &TomlValue) -> ModResult<bool> {
		if key.name != self.name {
			return Ok(false);
		}
		let array = value.as_array()?;

		match &self.value {
			Some((_, prev_loc, _)) if !self.mergable => {
				return Err(diagnostics::multiple_definitions(key.loc(), prev_loc, &self.name).into());
			}
			_ => (),
		}
		let (values, _, _) =
			self
				.value
				.get_or_insert((Vec::new(), key.loc().clone(), value.loc().clone()));

		for inner_value in array {
			values.push((self.parse_entry_fn)(&inner_value)?);
		}

		Ok(true)
	}
}

macro_rules! parse_table {
	($table:expr => [$($opt:expr),*], docs-pref: $docs_pref:expr $(,)?) => {'blk: {
		use $crate::parse::lib::*;
		for (key, value) in $table.iter() {
			let mut eaten = false;
			$(
				let wants_to_eat = match $opt.try_eat(&key, &value) {
					Ok(val) => val,
					Err(err) => break 'blk Err(err),
				};
				if !eaten && wants_to_eat {
					eaten = true;
				} else if eaten && wants_to_eat {
					panic!("multiple config options want to eat the same key");
				}
			)*
			if !eaten {
				break 'blk Result::Err(diagnostics::unknown_option(&key, $docs_pref).into());
			}
		}
		Ok(())
	}};
}
pub(crate) use parse_table;

pub mod diagnostics {
	use super::*;

	pub fn failed_canonicalization(value: &TomlValue, err: &CanonicalizationError) -> Diagnostic {
		let resolve_relative_span = |relative_span: &Range<usize>| {
			let mut loc = value.loc().clone();

			let base_span = loc.span;
			loc.span.start = base_span.start + relative_span.start;
			loc.span.end = base_span.start + relative_span.end;
			assert!(loc.span.end <= base_span.end);

			loc
		};
		let convert_label = |label: &CanonicalizationLabel| -> DiagLabel<usize> {
			let loc = label
				.span
				.as_ref()
				.map(resolve_relative_span)
				.unwrap_or(value.loc().clone());
			DiagLabel::new(label.ty, loc.file.0, loc.span).with_message(&label.message)
		};

		Diagnostic::new(Severity::Error)
			.with_message(&err.main_message)
			.with_labels(err.labels.iter().map(convert_label).collect())
			.with_notes(err.notes.clone())
	}
	pub fn missing_option(loc: &Location, missing: &str, docs_pref: &str) -> Diagnostic {
		let docs_url = format!("{DOCS_URL}#{docs_pref}");
		let label = loc.get_primary_label();
		Diagnostic::new(Severity::Error)
			.with_message(format!("missing config option `{missing}`"))
			.with_labels(vec![label])
			.with_notes(vec![format!("(see {docs_url})")])
	}
	pub fn unknown_option(key: &TomlKey, docs_pref: impl Into<String>) -> Diagnostic {
		let docs_url = format!("{DOCS_URL}#{}", docs_pref.into());
		Diagnostic::new(Severity::Error)
			.with_message("unknown config option")
			.with_labels(vec![key.loc().get_primary_label()])
			.with_notes(vec![format!("(see {docs_url} for supported options)")])
	}
	pub fn multiple_definitions(loc1: &Location, loc2: &Location, name: &str) -> Diagnostic {
		let label1 = loc2.get_primary_label().with_message("redefined here");
		let label2 = loc1
			.get_secondary_label()
			.with_message("first defined here");
		Diagnostic::new(Severity::Error)
			.with_message(format!("`{name}` is defined multiple times"))
			.with_labels(vec![label1, label2])
	}
	pub fn wrong_type(got: &TomlValue, expected: &[TomlInnerValue]) -> Diagnostic {
		assert!(!expected.is_empty());
		let expected_types_str = expected
			.iter()
			.map(|ty| format!("`{}`", toml_type_to_string(ty)))
			.collect::<Vec<_>>()
			.join(" or ");

		let label = got.loc().get_primary_label().with_message(format!(
			"expected {expected_types_str}, found `{}`",
			toml_type_to_string(got.value)
		));
		Diagnostic::new(Severity::Error)
			.with_message("unexpected type")
			.with_labels(vec![label])
	}
	fn toml_type_to_string(val: &TomlInnerValue) -> String {
		match val {
			TomlInnerValue::String(_) => "String",
			TomlInnerValue::Integer(_) => "Integer",
			TomlInnerValue::Float(_) => "Float",
			TomlInnerValue::Boolean(_) => "Boolean",
			TomlInnerValue::Array(_) => "Array",
			TomlInnerValue::Table(_) => "Table",
		}
		.to_string()
	}
}
