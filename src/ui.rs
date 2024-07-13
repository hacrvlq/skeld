use std::{
	error::Error,
	io::{self, Write},
};

use crossterm::{
	cursor,
	event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
	style, terminal,
	tty::IsTty as _,
	QueueableCommand as _,
};

pub use crossterm::style::Color;

#[derive(Clone)]
pub struct UiContent<U> {
	pub banner: String,
	pub sections: Vec<Section<U>>,
	pub colorscheme: Colorscheme,
}
#[derive(Clone)]
pub struct Colorscheme {
	pub neutral: Color,
	pub banner: Color,
	pub heading: Color,
	pub keybind: Color,
	pub button_label: Color,
}
#[derive(Clone)]
pub struct Section<U> {
	pub heading: String,
	pub buttons: Vec<Button<U>>,
}
#[derive(Clone)]
pub struct Button<U> {
	pub keybind: String,
	pub text: String,
	pub action: U,
}

#[derive(Debug, derive_more::From, derive_more::Display)]
pub enum UiError {
	#[display(fmt = "Error: skeld can only be used in a tty")]
	NoTty,
	#[display(fmt = "IO Error while displaying UI: {_0}")]
	IoError(io::Error),
}
impl Error for UiError {}

pub enum UserSelection<U> {
	Button(U),
	ControlC,
}
pub fn start<U: Clone>(data: &UiContent<U>) -> Result<UserSelection<U>, UiError> {
	if !io::stdout().is_tty() {
		return Err(UiError::NoTty);
	}

	terminal::enable_raw_mode()?;
	io::stdout()
		.queue(terminal::EnterAlternateScreen)?
		.queue(terminal::DisableLineWrap)?
		.queue(cursor::SavePosition)?
		.flush()?;
	let restore_terminal = || -> io::Result<()> {
		terminal::disable_raw_mode()?;
		io::stdout()
			.queue(terminal::LeaveAlternateScreen)?
			.queue(terminal::EnableLineWrap)?
			.queue(cursor::RestorePosition)?
			.queue(cursor::Show)?
			.flush()?;
		Ok(())
	};

	let mut state = State {
		data,
		selected_button: 0,
		acc_pressed_keys: String::new(),
	};

	loop {
		state.render()?;

		//TODO: add mouse support
		match event::read()? {
			Event::Key(KeyEvent {
				kind: KeyEventKind::Press,
				code: KeyCode::Char('c'),
				modifiers: KeyModifiers::CONTROL,
				..
			}) => {
				restore_terminal()?;
				return Ok(UserSelection::ControlC);
			}
			Event::Key(KeyEvent {
				kind: KeyEventKind::Press | KeyEventKind::Repeat,
				code,
				..
			}) => {
				let choosen_button_action = state.handle_key_press(&code);
				if let Some(data) = choosen_button_action {
					restore_terminal()?;
					return Ok(UserSelection::Button(data));
				}
			}
			_ => (),
		}
	}
}

struct State<'a, U> {
	data: &'a UiContent<U>,
	selected_button: usize,
	// accumulated pressed keys
	// (never cleared, only the end is checked for a match)
	acc_pressed_keys: String,
}

impl<U: Clone> State<'_, U> {
	fn render(&self) -> io::Result<()> {
		assert!(terminal::is_raw_mode_enabled()?);

		let mut renderer = CenteredTextRenderer::new();

		renderer.push_text(&self.data.banner, self.data.colorscheme.banner);
		renderer.push_text("\n\n\n", Color::Reset);

		let mut possible_cursor_positions = Vec::new();
		for section in &self.data.sections {
			renderer.push_text(&section.heading, self.data.colorscheme.heading);
			renderer.push_text("\n\n", Color::Reset);
			for button in &section.buttons {
				possible_cursor_positions.push((1, renderer.get_line_count() as u16));
				button.render(&self.data.colorscheme, &mut renderer);
			}
			renderer.push_text("\n\n", Color::Reset);
		}

		let cursor_pos = *possible_cursor_positions
			.get(self.selected_button)
			.unwrap_or(&(u16::MAX, 0));
		renderer.render(cursor_pos)?;

		Ok(())
	}
	fn handle_key_press(&mut self, keycode: &KeyCode) -> Option<U> {
		match keycode {
			KeyCode::Enter => {
				return self
					.buttons()
					.nth(self.selected_button)
					.map(|button| button.action.clone());
			}
			KeyCode::Char('j') | event::KeyCode::Down => {
				let max_idx = self.buttons().count().saturating_sub(1);
				self.selected_button = (self.selected_button + 1).min(max_idx);
			}
			KeyCode::Char('k') | event::KeyCode::Up => {
				self.selected_button = self.selected_button.saturating_sub(1);
			}
			KeyCode::Char(ch) => {
				self.acc_pressed_keys.push(*ch);
				return self.check_for_keybind_match();
			}
			_ => (),
		}

		None
	}
	fn check_for_keybind_match(&self) -> Option<U> {
		let pressed_button = self
			.buttons()
			.filter(|button| self.acc_pressed_keys.ends_with(&button.keybind))
			.max_by_key(|button| button.keybind.len());
		pressed_button.map(|button| button.action.clone())
	}
	fn buttons(&self) -> impl Iterator<Item = &Button<U>> {
		self
			.data
			.sections
			.iter()
			.flat_map(|section| section.buttons.iter())
	}
}

impl<U> Button<U> {
	fn render(&self, colorscheme: &Colorscheme, renderer: &mut CenteredTextRenderer) {
		renderer.push_text("[", colorscheme.neutral);
		renderer.push_text(&self.keybind, colorscheme.keybind);
		renderer.push_text("] ", colorscheme.neutral);
		renderer.push_text(&self.text, colorscheme.button_label);
		renderer.push_text("\n", Color::Reset);
	}
}

struct CenteredTextRenderer {
	text: String,
	max_text_width: usize,
	line_count: usize,
}
impl CenteredTextRenderer {
	fn new() -> Self {
		Self {
			text: String::new(),
			max_text_width: 0,
			line_count: 0,
		}
	}
	fn get_line_count(&self) -> usize {
		self.line_count
	}
	fn push_text(&mut self, text: &str, color: Color) {
		use style::Stylize;

		self.text.push_str(&text.with(color).to_string());
		self.max_text_width = self
			.max_text_width
			.max(text.lines().map(|line| line.len()).max().unwrap_or(0));
		self.line_count += text.chars().filter(|ch| ch == &'\n').count();
	}
	fn render(&self, cursor_pos: (u16, u16)) -> std::io::Result<()> {
		let mut stdout = io::stdout();

		let terminal_size = terminal::size()?;
		let leftmost_pos =
			((terminal_size.0 as f32 - self.max_text_width as f32).max(0.0) * 0.5) as u16;

		stdout.queue(terminal::Clear(terminal::ClearType::All))?;
		for (i, line) in self.text.lines().enumerate().take(terminal_size.1 as usize) {
			stdout
				.queue(cursor::MoveTo(leftmost_pos, i as u16))?
				.queue(style::Print(&line))?;
		}

		if cursor_pos.0 < terminal_size.0 && cursor_pos.1 < terminal_size.1 {
			stdout.queue(cursor::Show)?;
			stdout.queue(cursor::MoveTo(leftmost_pos + cursor_pos.0, cursor_pos.1))?;
		} else {
			stdout.queue(cursor::Hide)?;
		}
		stdout.flush()?;

		Ok(())
	}
}
