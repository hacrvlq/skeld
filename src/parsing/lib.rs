use std::{
	borrow::Cow,
	cmp::PartialEq,
	fs,
	ops::{Bound as RangeBound, Range, RangeBounds},
	path::{Path, PathBuf},
	rc::Rc,
};

use codespan_reporting::{
	diagnostic::{self, Label as DiagLabel, LabelStyle as DiagLabelStyle},
	files as codespan_files,
};
use toml_span::Span;

use super::ModResult;
use crate::vec_ext::VecExt as _;

pub use codespan_reporting::diagnostic::Severity;
pub use toml_span::value::ValueInner as TomlInnerValue;
pub type Diagnostic = diagnostic::Diagnostic<usize>;
pub type FileDatabase = codespan_files::SimpleFiles<String, String>;

// trait implementors should save current config value
// and update it with each 'eat_with_user_data'
pub trait ConfigOption {
	type ParsedKey;
	type UserData: Default;

	// should return 'Some' when 'key' is to be processed by this option
	fn would_eat(&self, key: &TomlKey) -> Option<Self::ParsedKey>;
	fn eat_with_user_data(
		&mut self,
		key: Self::ParsedKey,
		value: TomlValue,
		user_data: Self::UserData,
	) -> ModResult<()>;

	fn eat(&mut self, key: Self::ParsedKey, value: TomlValue) -> ModResult<()> {
		self.eat_with_user_data(key, value, Self::UserData::default())
	}
}

// =================================================================================================
// Wrappers around 'toml_span' adding file location info
// =================================================================================================
pub fn parse_toml_file<'v>(
	path: impl AsRef<Path>,
	file_database: &mut FileDatabase,
	// file contents and root toml value need to outlive the return value
	outlivers: &'v mut Option<String>,
) -> ModResult<TomlTable<'v>> {
	let path = path.as_ref();
	assert!(path.is_absolute());

	*outlivers = Some(
		fs::read_to_string(path)
			.map_err(|err| format!("Failed to read file `{}`: {err}", path.display()))?,
	);
	let file_contents = outlivers.as_ref().unwrap();

	let file_id =
		FileId(file_database.add(path.to_string_lossy().to_string(), file_contents.clone()));

	let mut parsed_contents =
		toml_span::parse(file_contents).map_err(|err| err.to_diagnostic(file_id.0))?;
	let TomlInnerValue::Table(table) = parsed_contents.take() else {
		unreachable!()
	};

	Ok(TomlTable {
		table,
		loc: Location {
			span: parsed_contents.span,
			file: file_id,
		},
	})
}
#[derive(Clone, Debug)]
pub struct TomlKey<'a> {
	name: Cow<'a, str>,
	loc: Location,
}
impl<'a> TomlKey<'a> {
	pub fn new_owned(name: String, loc: Location) -> TomlKey<'static> {
		TomlKey {
			name: name.into(),
			loc,
		}
	}
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
#[derive(Debug)]
pub struct TomlValue<'a> {
	value: TomlInnerValue<'a>,
	loc: Location,
}
impl<'a> TomlValue<'a> {
	fn from_value(mut value: toml_span::Value<'a>, file: FileId) -> Self {
		Self {
			value: value.take(),
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
	pub fn into_array(self) -> ModResult<Vec<TomlValue<'a>>> {
		let file = self.loc().file;

		let TomlInnerValue::Array(array) = self.value else {
			return Err(
				diagnostics::wrong_type(&self, &[TomlInnerValue::Table(Default::default())]).into(),
			);
		};

		let array = array
			.into_iter()
			.map(|value| TomlValue::from_value(value, file))
			.collect();

		Ok(array)
	}
	pub fn into_table(self) -> ModResult<TomlTable<'a>> {
		let TomlInnerValue::Table(table) = self.value else {
			return Err(
				diagnostics::wrong_type(&self, &[TomlInnerValue::Table(Default::default())]).into(),
			);
		};
		Ok(TomlTable {
			table,
			loc: self.loc,
		})
	}
}
pub struct TomlTable<'a> {
	table: toml_span::value::Table<'a>,
	loc: Location,
}
impl<'a> TomlTable<'a> {
	pub fn into_iter(self) -> impl Iterator<Item = (TomlKey<'a>, TomlValue<'a>)> {
		let file_id = self.loc().file;
		self.table.into_iter().map(move |(key, value)| {
			(
				TomlKey::from_key(&key, file_id),
				TomlValue::from_value(value, file_id),
			)
		})
	}
	pub fn remove_entry(&mut self, key_name: &str) -> Option<(TomlKey<'a>, TomlValue<'a>)> {
		let search_key = toml_span::value::Key {
			name: key_name.to_owned().into(),
			span: Default::default(),
		};
		self.table.remove_entry(&search_key).map(|(key, value)| {
			(
				TomlKey::from_key(&key, self.loc.file),
				TomlValue::from_value(value, self.loc.file),
			)
		})
	}
	pub fn loc(&self) -> &Location {
		&self.loc
	}
}

#[derive(Clone, Debug)]
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
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Priority(pub i64);

// helper config option that eats only a specific key using a specified parse function
#[derive(Clone, derive_more::Debug)]
pub struct BaseOption<'a, T> {
	#[debug(skip)]
	parse_fn: Rc<dyn Fn(TomlValue) -> ModResult<T> + 'a>,
	name: String,
	parsed_values: Vec<(T, Location, Priority)>,
}
impl<'a, T> BaseOption<'a, T> {
	pub fn new(name: &str, parse_fn: impl Fn(TomlValue) -> ModResult<T> + 'a) -> Self {
		Self {
			parse_fn: Rc::new(parse_fn),
			name: name.to_string(),
			parsed_values: Vec::new(),
		}
	}
	pub fn get_value(self) -> ModResult<Option<T>> {
		let max_prio_values = self.parsed_values.get_maximums_by_key(|(_, _, prio)| *prio);

		if max_prio_values.len() >= 2 {
			return Err(
				diagnostics::multiple_definitions(&max_prio_values[0].1, &max_prio_values[1].1, &self.name)
					.into(),
			);
		}

		let max_prio_value = max_prio_values.into_iter().next();
		Ok(max_prio_value.map(|(value, _, _)| value))
	}
}
impl<T: PartialEq> ConfigOption for BaseOption<'_, T> {
	type ParsedKey = Location;
	type UserData = Priority;

	fn would_eat(&self, key: &TomlKey) -> Option<Self::ParsedKey> {
		(key.name() == self.name).then(|| key.loc().clone())
	}

	fn eat_with_user_data(
		&mut self,
		key_loc: Self::ParsedKey,
		value: TomlValue,
		priority: Self::UserData,
	) -> ModResult<()> {
		let value = (self.parse_fn)(value)?;
		self.parsed_values.push((value, key_loc, priority));
		Ok(())
	}
}

macro_rules! wrap_BaseOption {
	($vis:vis $name:ident : $inner_type:ty) => {
		#[derive(std::clone::Clone, std::fmt::Debug)]
		$vis struct $name<'a>($crate::parsing::lib::BaseOption<'a, $inner_type>);

		impl<'a> $crate::parsing::lib::ConfigOption for $name<'a> {
			type ParsedKey = <$crate::parsing::lib::BaseOption<'a, $inner_type> as $crate::parsing::lib::ConfigOption>::ParsedKey;
			type UserData = <$crate::parsing::lib::BaseOption<'a, $inner_type> as $crate::parsing::lib::ConfigOption>::UserData;

			fn would_eat(&self, key: &$crate::parsing::lib::TomlKey) -> Option<Self::ParsedKey> {
				self.0.would_eat(key)
			}

			fn eat_with_user_data(
				&mut self,
				key: Self::ParsedKey,
				value: $crate::parsing::lib::TomlValue,
				user_data: Self::UserData,
			) -> $crate::GenericResult<()> {
				self.0.eat_with_user_data(key, value, user_data)
			}
			fn eat(
				&mut self,
				key: Self::ParsedKey,
				value: $crate::parsing::lib::TomlValue,
			) -> $crate::GenericResult<()> {
				self.0.eat(key, value)
			}
		}
	};
}
pub(crate) use wrap_BaseOption;

wrap_BaseOption!(pub BoolOption : bool);
impl BoolOption<'_> {
	pub fn new(name: &str) -> Self {
		Self(BaseOption::new(name, |value| value.as_bool()))
	}
	pub fn get_value(self) -> ModResult<Option<bool>> {
		self.0.get_value()
	}
}

wrap_BaseOption!(pub IntegerOption : i64);
impl<'a> IntegerOption<'a> {
	pub fn new(name: &str, valid_range: impl RangeBounds<i64> + 'a) -> Self {
		Self(BaseOption::new(name, move |raw_value| {
			let value = raw_value.as_int()?;

			let RangeBound::Included(lower_bound) = valid_range.start_bound() else {
				todo!()
			};
			let RangeBound::Included(upper_bound) = valid_range.end_bound() else {
				todo!()
			};

			if !valid_range.contains(&value) {
				return Err(
					Diagnostic::new(Severity::Error)
						.with_message("number is out of range")
						.with_label(raw_value.loc().get_primary_label().with_message(format!(
							"must be within the range {lower_bound} <= _ <= {upper_bound}"
						)))
						.into(),
				);
			}

			Ok(value)
		}))
	}
	pub fn get_value(self) -> ModResult<Option<i64>> {
		self.0.get_value()
	}
}

// useful to suppress "unknown option" errors
#[derive(Clone, Debug)]
pub struct MockOption {
	name: String,
}
impl MockOption {
	pub fn new(name: &str) -> Self {
		Self {
			name: name.to_string(),
		}
	}
}
impl ConfigOption for MockOption {
	type ParsedKey = ();
	type UserData = ();
	fn would_eat(&self, key: &TomlKey) -> Option<Self::ParsedKey> {
		(key.name == self.name).then_some(())
	}
	fn eat_with_user_data(
		&mut self,
		_key: Self::ParsedKey,
		_value: TomlValue,
		_user_data: Self::UserData,
	) -> ModResult<()> {
		Ok(())
	}
}

wrap_BaseOption!(pub PathBufOption : PathBuf);
impl<'a> PathBufOption<'a> {
	pub fn new(
		name: &str,
		canonicalization: impl Fn(&str) -> Result<PathBuf, CanonicalizationError> + 'a,
	) -> Self {
		Self(BaseOption::new(name, move |value| {
			let raw_value = value.as_str()?;
			(canonicalization)(raw_value)
				.map_err(|err| diagnostics::failed_canonicalization(value.loc(), &err).into())
		}))
	}
	pub fn get_value(self) -> ModResult<Option<PathBuf>> {
		self.0.get_value()
	}
}
wrap_BaseOption!(pub StringOption : String);
impl<'a> StringOption<'a> {
	pub fn new(name: &str) -> Self {
		Self::new_with_canonicalization(name, |str| Ok(str.to_string()))
	}
	pub fn new_with_canonicalization(
		name: &str,
		canonicalization: impl Fn(&str) -> Result<String, CanonicalizationError> + 'a,
	) -> Self {
		Self(BaseOption::new(name, move |value| {
			let raw_value = value.as_str()?;
			(canonicalization)(raw_value)
				.map_err(|err| diagnostics::failed_canonicalization(value.loc(), &err).into())
		}))
	}
	pub fn get_value(self) -> ModResult<Option<String>> {
		self.0.get_value()
	}
}

#[derive(Clone, Debug)]
pub struct CanonicalizationError {
	pub main_message: String,
	pub labels: Vec<CanonicalizationLabel>,
	pub notes: Vec<String>,
}
#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
pub struct ArrayOption<V> {
	name: String,
	// value: Option<(_, key location, value location)>
	value: Option<(Vec<V>, Location, Location)>,
	parse_entry_fn: fn(TomlValue) -> ModResult<V>,
	mergable: bool,
}
impl<V> ArrayOption<V> {
	pub fn new(name: &str, mergable: bool, parse_entry_fn: fn(TomlValue) -> ModResult<V>) -> Self {
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
	type ParsedKey = Location;
	type UserData = ();

	fn would_eat(&self, key: &TomlKey) -> Option<Self::ParsedKey> {
		(key.name == self.name).then(|| key.loc().clone())
	}
	fn eat_with_user_data(
		&mut self,
		key_loc: Self::ParsedKey,
		value: TomlValue,
		_user_data: Self::UserData,
	) -> ModResult<()> {
		let value_loc = value.loc().clone();
		let array = value.into_array()?;

		match &self.value {
			Some((_, prev_loc, _)) if !self.mergable => {
				return Err(diagnostics::multiple_definitions(&key_loc, prev_loc, &self.name).into());
			}
			_ => (),
		}
		let (values, _, _) = self
			.value
			.get_or_insert_with(|| (Vec::new(), key_loc, value_loc));

		for inner_value in array {
			values.push((self.parse_entry_fn)(inner_value)?);
		}

		Ok(())
	}
}

macro_rules! parse_table {
	($table:expr => [$($opt:expr $(; $data:expr)?),*], docs-section: $docs_section:expr $(,)?) => { 'ret: {
		for (key, value) in $table.into_iter() {
			let result = 'inner_blk: {
				$(
					#[allow(clippy::allow_attributes, unused_imports)]
					use $crate::parsing::lib::ConfigOption as _;
					if let Some(parsed_key) = $opt.would_eat(&key) {
						#[allow(clippy::allow_attributes, unused_mut, unused_assignments)]
						let mut user_data = std::default::Default::default();
						$( user_data = $data; )?

						break 'inner_blk $opt.eat_with_user_data(parsed_key, value, user_data);
					}
				)*
				std::result::Result::Err(
					$crate::parsing::lib::diagnostics::unknown_option(&key, $docs_section).into()
				)
			};
			if result.is_err() {
				break 'ret result;
			}
		}
		Ok(())
	}};
}
pub(crate) use parse_table;

pub mod diagnostics {
	use super::*;

	pub fn failed_canonicalization(value_loc: &Location, err: &CanonicalizationError) -> Diagnostic {
		let resolve_relative_span = |relative_span: &Range<usize>| {
			let mut loc = value_loc.clone();

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
				.unwrap_or_else(|| value_loc.clone());
			DiagLabel::new(label.ty, loc.file.0, loc.span).with_message(&label.message)
		};

		Diagnostic::new(Severity::Error)
			.with_message(&err.main_message)
			.with_labels(err.labels.iter().map(convert_label).collect())
			.with_notes(err.notes.clone())
	}
	pub fn missing_option(loc: &Location, missing: &str, docs_section: &str) -> Diagnostic {
		let label = loc.get_primary_label();
		Diagnostic::new(Severity::Error)
			.with_message(format!("missing config option `{missing}`"))
			.with_labels(vec![label])
			.with_notes(vec![format!(
				"(run `{man_cmd}` for more information)",
				man_cmd = crate::error::get_manpage_cmd(docs_section),
			)])
	}
	pub fn unknown_option(key: &TomlKey, docs_section: &str) -> Diagnostic {
		Diagnostic::new(Severity::Error)
			.with_message("unknown config option")
			.with_labels(vec![key.loc().get_primary_label()])
			.with_notes(vec![format!(
				"(run `{man_cmd}` to see all supported options)",
				man_cmd = crate::error::get_manpage_cmd(docs_section),
			)])
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
			.map(|ty| format!("`{}`", ty.type_str()))
			.collect::<Vec<_>>()
			.join(" or ");

		let label = got.loc().get_primary_label().with_message(format!(
			"expected {expected_types_str}, found `{}`",
			got.value.type_str()
		));
		Diagnostic::new(Severity::Error)
			.with_message("unexpected type")
			.with_labels(vec![label])
	}
}
