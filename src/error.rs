use std::{convert::From, error::Error, io};

use codespan_reporting::term::{self, termcolor};
use crossterm::tty::IsTty as _;

use crate::parse::FileDatabase;

pub use crate::parse::Diagnostic;

// alternative to Box<dyn Error>, which also supports Diagnostic's
#[derive(Debug, derive_more::From)]
pub enum GenericError {
	Diagnostic(Diagnostic),
	Generic(Box<dyn Error>),
}
pub type GenericResult<T> = Result<T, GenericError>;

impl GenericError {
	pub fn print(&self, files: &FileDatabase) {
		match self {
			GenericError::Diagnostic(diag) => Self::print_diagnostic(diag, files),
			GenericError::Generic(msg) => eprintln!("{msg}"),
		}
	}
	fn print_diagnostic(diag: &Diagnostic, files: &FileDatabase) {
		let color_choice = if io::stderr().is_tty() {
			termcolor::ColorChoice::Auto
		} else {
			termcolor::ColorChoice::Never
		};
		let writer = termcolor::StandardStream::stderr(color_choice);
		let config = term::Config::default();
		let writer_error = term::emit(&mut writer.lock(), &config, files, diag);
		if writer_error.is_err() {
			eprintln!(
				"Failed to pretty-print error, here is the raw version:\nerror: {}",
				diag.message
			);
		}
	}
}

impl From<&str> for GenericError {
	fn from(value: &str) -> Self {
		Self::Generic(value.into())
	}
}
impl From<String> for GenericError {
	fn from(value: String) -> Self {
		Self::Generic(value.into())
	}
}

// get cmd to open the skeld manpage at 'section'
pub fn get_manpage_cmd(section: &str) -> String {
	let section_str = if section.contains(|ch: char| ch.is_whitespace()) {
		format!("\"{section}\"")
	} else {
		section.to_string()
	};
	format!("man -P 'less -p {section_str}$' skeld")
}
