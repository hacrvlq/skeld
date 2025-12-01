use std::{convert::From, error::Error, io};

use codespan_reporting::term::{self, termcolor};
use crossterm::tty::IsTty as _;

use crate::parsing::FileDatabase;

pub use crate::parsing::Diagnostic;

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
