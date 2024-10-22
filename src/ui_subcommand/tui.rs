use std::{
	error::Error,
	io::{self, Write},
	ops::RangeInclusive,
	time,
};

use crossterm::{
	cursor,
	event::{
		self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
		MouseEventKind,
	},
	style, terminal,
	tty::IsTty as _,
	QueueableCommand as _,
};
use unicode_width::UnicodeWidthStr;

pub use crossterm::style::Color;

#[derive(Clone)]
pub struct TuiData<U> {
	pub banner: String,
	pub sections: Vec<Section<U>>,
	pub colorscheme: Colorscheme,
}
#[derive(Clone, PartialEq, Eq)]
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
	#[display("The skeld ui can only be used in a tty.")]
	NoTty,
	#[display("An IO error occurred while rendering the tui: {_0}")]
	IoError(io::Error),
}
impl Error for UiError {}

pub enum UserSelection<U> {
	Button(U),
	ControlC,
}
pub fn run<U: Clone>(data: &TuiData<U>) -> Result<UserSelection<U>, UiError> {
	if !io::stdout().is_tty() {
		return Err(UiError::NoTty);
	}

	terminal::enable_raw_mode()?;
	io::stdout()
		.queue(terminal::EnterAlternateScreen)?
		.queue(event::EnableMouseCapture)?
		.queue(terminal::DisableLineWrap)?
		.queue(cursor::SavePosition)?
		.flush()?;
	let restore_terminal = || -> io::Result<()> {
		terminal::disable_raw_mode()?;
		io::stdout()
			.queue(terminal::LeaveAlternateScreen)?
			.queue(event::DisableMouseCapture)?
			.queue(terminal::EnableLineWrap)?
			.queue(cursor::RestorePosition)?
			.queue(cursor::Show)?
			.flush()?;
		Ok(())
	};

	let mut state = State {
		data,
		rendered_content: RenderedContent::new(data)?,
		selected_button: 0,
		acc_pressed_keys: String::new(),
		prev_mouse_press: None,
	};

	loop {
		if terminal::size()? != state.rendered_content.terminal_size {
			state.rendered_content = RenderedContent::new(state.data)?;
		}
		state.rendered_content.display(state.selected_button)?;

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
			event => {
				let choosen_button_action = state.handle_event(&event);
				if let Some(action) = choosen_button_action {
					restore_terminal()?;
					return Ok(UserSelection::Button(action));
				}
			}
		}
	}
}

struct State<'a, U> {
	data: &'a TuiData<U>,
	rendered_content: RenderedContent,
	selected_button: usize,
	// accumulated pressed keys
	// (never cleared, only the end is checked for a match)
	acc_pressed_keys: String,
	// prev_mouse_press: Option<(pressed button, _)>
	prev_mouse_press: Option<(usize, time::Instant)>,
}

impl<U: Clone> State<'_, U> {
	fn handle_event(&mut self, event: &Event) -> Option<U> {
		match event {
			Event::Key(KeyEvent {
				kind: KeyEventKind::Press | KeyEventKind::Repeat,
				code,
				..
			}) => self.handle_key_press(*code),
			Event::Mouse(MouseEvent {
				kind: MouseEventKind::Down(MouseButton::Left),
				column,
				row,
				..
			}) => self.handle_mouse_press((*column, *row)),
			_ => None,
		}
	}
	fn handle_key_press(&mut self, keycode: KeyCode) -> Option<U> {
		if let KeyCode::Char(ch) = keycode {
			self.acc_pressed_keys.push(ch);
		}

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
			_ => (),
		};

		self.check_for_keybind_match()
	}
	fn check_for_keybind_match(&self) -> Option<U> {
		let pressed_button = self
			.buttons()
			.filter(|button| self.acc_pressed_keys.ends_with(&button.keybind))
			.max_by_key(|button| button.keybind.len());
		pressed_button.map(|button| button.action.clone())
	}

	fn handle_mouse_press(&mut self, pos: (u16, u16)) -> Option<U> {
		let now = time::Instant::now();

		let Some(pressed_button) = self
			.rendered_content
			.buttons_clickable_area
			.iter()
			.position(|(line, col_range)| line == &pos.1 && col_range.contains(&pos.0))
		else {
			self.prev_mouse_press = None;
			return None;
		};

		const DOUBLE_CLICK_TIME: f64 = 0.5;
		if self
			.prev_mouse_press
			.as_ref()
			.is_some_and(|(prev_button, prev_time)| {
				prev_button == &pressed_button && (now - *prev_time).as_secs_f64() < DOUBLE_CLICK_TIME
			}) {
			self.prev_mouse_press = None;
			Some(self.buttons().nth(pressed_button).unwrap().action.clone())
		} else {
			self.selected_button = pressed_button;
			self.prev_mouse_press = Some((pressed_button, now));
			None
		}
	}

	fn buttons(&self) -> impl Iterator<Item = &Button<U>> {
		self
			.data
			.sections
			.iter()
			.flat_map(|section| section.buttons.iter())
	}
}

// styled and layouted text of the tui
struct RenderedContent {
	// terminal size at the time of creation
	terminal_size: (u16, u16),
	text: String,
	left_padding: u16,
	// buttons_clickable_area: Vec<(line, row_range)>
	buttons_clickable_area: Vec<(u16, RangeInclusive<u16>)>,
}
impl RenderedContent {
	fn new<U>(content: &TuiData<U>) -> io::Result<Self> {
		let mut text = TextBuilder::new();

		text.push_text(&content.banner, content.colorscheme.banner);
		text.push_text("\n\n\n", Color::Reset);

		let mut buttons_clickable_area = Vec::new();
		for section in &content.sections {
			text.push_text(&section.heading, content.colorscheme.heading);
			text.push_text("\n\n", Color::Reset);
			for button in &section.buttons {
				buttons_clickable_area.push((text.line_count as u16, 0..=button.keybind.len() as u16 + 1));
				button.render(&content.colorscheme, &mut text);
			}
			text.push_text("\n\n", Color::Reset);
		}

		let terminal_size = terminal::size()?;
		let left_padding =
			((terminal_size.0 as f32 - text.max_text_width as f32).max(0.0) * 0.5) as u16;

		let buttons_clickable_area = buttons_clickable_area
			.into_iter()
			.map(|(line, range)| {
				(
					line,
					*range.start() + left_padding..=*range.end() + left_padding,
				)
			})
			.collect();

		Ok(Self {
			terminal_size,
			left_padding,
			text: text.text,
			buttons_clickable_area,
		})
	}
	fn display(&self, selected_button: usize) -> io::Result<()> {
		assert!(terminal::is_raw_mode_enabled()?);

		let mut stdout = io::stdout();

		stdout.queue(terminal::Clear(terminal::ClearType::All))?;
		for (i, line) in self
			.text
			.lines()
			.enumerate()
			.take(self.terminal_size.1 as usize)
		{
			stdout
				.queue(cursor::MoveTo(self.left_padding, i as u16))?
				.queue(style::Print(&line))?;
		}

		let cursor_pos = self
			.buttons_clickable_area
			.get(selected_button)
			.map(|(line, range)| (*range.start() + 1, *line))
			.unwrap_or((u16::MAX, u16::MAX));
		if cursor_pos.0 < self.terminal_size.0 && cursor_pos.1 < self.terminal_size.1 {
			stdout.queue(cursor::Show)?;
			stdout.queue(cursor::MoveTo(cursor_pos.0, cursor_pos.1))?;
		} else {
			stdout.queue(cursor::Hide)?;
		}
		stdout.flush()?;

		Ok(())
	}
}

impl<U> Button<U> {
	fn render(&self, colorscheme: &Colorscheme, out: &mut TextBuilder) {
		out.push_text("[", colorscheme.neutral);
		out.push_text(&self.keybind, colorscheme.keybind);
		out.push_text("] ", colorscheme.neutral);
		out.push_text(&self.text, colorscheme.button_label);
		out.push_text("\n", Color::Reset);
	}
}

// record the maximum width of the text
// that may be styled with ansi escape sequences
struct TextBuilder {
	text: String,
	max_text_width: usize,
	line_count: usize,
}
impl TextBuilder {
	fn new() -> Self {
		Self {
			text: String::new(),
			max_text_width: 0,
			line_count: 0,
		}
	}
	fn push_text(&mut self, text: &str, color: Color) {
		use style::Stylize;

		self.text.push_str(&text.with(color).to_string());
		self.max_text_width = self
			.max_text_width
			.max(text.lines().map(UnicodeWidthStr::width).max().unwrap_or(0));
		self.line_count += text.chars().filter(|ch| ch == &'\n').count();
	}
}
