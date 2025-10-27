use std::{
	io::{self, Write as _},
	panic,
	process::ExitCode,
	time,
};

use crossterm::{
	ExecutableCommand as _, QueueableCommand as _, cursor,
	event::{
		self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
		MouseEventKind,
	},
	terminal,
	tty::IsTty as _,
};

use crate::{
	GenericError,
	parsing::{ParseContext, RawProjectData},
	project::ProjectDataFile,
};

pub use crossterm::style::Color;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Colorscheme {
	pub normal: Color,
	pub banner: Color,
	pub heading: Color,
	pub keybind: Color,
	pub project_name: Color,
	pub background: Color,
}

#[derive(Clone, Debug)]
pub(super) struct TuiData {
	pub(super) colorscheme: Colorscheme,
	pub(super) banner: String,
	pub(super) sections: Vec<ProjectsSection>,
	pub(super) project_button_width: u32,
}
#[derive(Clone, Debug)]
pub(super) struct ProjectsSection {
	pub(super) heading: String,
	pub(super) buttons: Vec<ProjectButton>,
}
#[derive(Clone, Debug)]
pub(super) struct ProjectButton {
	pub(super) keybind: String,
	pub(super) project_name: String,
	pub(super) project: ProjectDataFile,
}

#[derive(Debug, derive_more::From)]
pub(super) enum UiError {
	IoError(io::Error),
	NoTty,
	Other(GenericError),
}
pub(super) fn run(
	tui_data: &TuiData,
	global_project_data: RawProjectData,
	parse_ctx: &mut ParseContext,
) -> Result<ExitCode, UiError> {
	if !io::stdout().is_tty() {
		return Err(UiError::NoTty);
	}

	setup_terminal().inspect_err(|_| restore_terminal())?;
	// restore the terminal before a panic is displayed
	let default_panic_hook = panic::take_hook();
	panic::set_hook(Box::new(move |info| {
		restore_terminal();
		default_panic_hook(info);
	}));

	let result = protected_run(tui_data, global_project_data, parse_ctx);

	restore_terminal();
	// revert to the default panic hook
	let _ = panic::take_hook();

	result
}

fn setup_terminal() -> io::Result<()> {
	terminal::enable_raw_mode()?;
	io::stdout()
		.queue(terminal::EnterAlternateScreen)?
		.queue(event::EnableMouseCapture)?
		.queue(terminal::DisableLineWrap)?
		.queue(cursor::SavePosition)?
		.flush()?;
	Ok(())
}
fn restore_terminal() {
	let mut stdout = io::stdout();
	let _ = terminal::disable_raw_mode();

	let _ = stdout.execute(terminal::LeaveAlternateScreen);
	let _ = stdout.execute(event::DisableMouseCapture);
	let _ = stdout.execute(terminal::EnableLineWrap);
	let _ = stdout.execute(cursor::RestorePosition);
	let _ = stdout.execute(cursor::Show);
}

fn protected_run(
	tui_data: &TuiData,
	global_project_data: RawProjectData,
	parse_ctx: &mut ParseContext,
) -> Result<ExitCode, UiError> {
	let mut state = UIState {
		tui_data,
		parse_ctx,
		global_project_data: Some(global_project_data),

		buttons_clickable_area: Vec::new(),
		selected_button: 0,
		acc_pressed_keys: String::new(),
		prev_mouse_press: None,
	};

	loop {
		let (rendered_content, button_areas) = renderer::render(&state)?;
		rendered_content.display(tui_data.colorscheme.background)?;
		state.buttons_clickable_area = button_areas;

		let event = event::read()?;
		let exit_code_opt = state.handle_event(event)?;
		if let Some(exit_code) = exit_code_opt {
			return Ok(exit_code);
		}
	}
}

struct UIState<'a, 'b, 'c> {
	tui_data: &'a TuiData,
	global_project_data: Option<RawProjectData>,
	parse_ctx: &'b mut ParseContext<'c>,

	buttons_clickable_area: Vec<renderer::Area>,
	selected_button: usize,
	// accumulated pressed keys
	// (never cleared, only the end is checked for a match)
	acc_pressed_keys: String,
	// prev_mouse_press: Option<(pressed button, _)>
	prev_mouse_press: Option<(usize, time::Instant)>,
}
impl UIState<'_, '_, '_> {
	fn handle_event(&mut self, event: Event) -> Result<Option<ExitCode>, UiError> {
		match event {
			Event::Key(KeyEvent {
				kind: KeyEventKind::Press | KeyEventKind::Repeat,
				code,
				modifiers,
				..
			}) => self.handle_key_press(code, modifiers),
			Event::Mouse(MouseEvent {
				kind: MouseEventKind::Down(MouseButton::Left),
				column,
				row,
				..
			}) => self.handle_mouse_press((column, row)),
			_ => Ok(None),
		}
	}
	fn handle_key_press(
		&mut self,
		keycode: KeyCode,
		modifiers: KeyModifiers,
	) -> Result<Option<ExitCode>, UiError> {
		if let KeyCode::Char(ch) = keycode {
			self.acc_pressed_keys.push(ch);
		}

		if keycode == KeyCode::Char('c') && modifiers == KeyModifiers::CONTROL {
			return Ok(Some(ExitCode::SUCCESS));
		}

		match keycode {
			KeyCode::Enter => {
				let selected_project = self
					.tui_data
					.buttons()
					.nth(self.selected_button)
					.map(|button| button.project.clone());

				if let Some(selected_project) = selected_project {
					return self.handle_selected_project(selected_project).map(Some);
				}
			}
			KeyCode::Char('j') | KeyCode::Down => {
				let max_idx = self.tui_data.buttons().count().saturating_sub(1);
				self.selected_button = (self.selected_button + 1).min(max_idx);
			}
			KeyCode::Char('k') | KeyCode::Up => {
				self.selected_button = self.selected_button.saturating_sub(1);
			}
			_ => (),
		}

		if let Some(selected_project) = self.check_for_keybind_match() {
			return self.handle_selected_project(selected_project).map(Some);
		}

		Ok(None)
	}
	fn check_for_keybind_match(&self) -> Option<ProjectDataFile> {
		let pressed_button = self
			.tui_data
			.buttons()
			.filter(|button| self.acc_pressed_keys.ends_with(&button.keybind))
			.max_by_key(|button| button.keybind.len());
		pressed_button.map(|button| button.project.clone())
	}

	fn handle_mouse_press(&mut self, pos: (u16, u16)) -> Result<Option<ExitCode>, UiError> {
		let now = time::Instant::now();

		let Some(pressed_button) = self
			.buttons_clickable_area
			.iter()
			.position(|area| area.contains(pos))
		else {
			self.prev_mouse_press = None;
			return Ok(None);
		};

		const DOUBLE_CLICK_TIME: f64 = 0.5;
		if self
			.prev_mouse_press
			.as_ref()
			.is_some_and(|(prev_button, prev_time)| {
				prev_button == &pressed_button && (now - *prev_time).as_secs_f64() < DOUBLE_CLICK_TIME
			}) {
			self.prev_mouse_press = None;

			let selected_project = self
				.tui_data
				.buttons()
				.nth(pressed_button)
				.unwrap()
				.project
				.clone();
			self.handle_selected_project(selected_project).map(Some)
		} else {
			self.selected_button = pressed_button;
			self.prev_mouse_press = Some((pressed_button, now));
			Ok(None)
		}
	}

	fn handle_selected_project(&mut self, project: ProjectDataFile) -> Result<ExitCode, UiError> {
		// NOTE: Since the TUI exits after a project is selected,
		//       'self.global_project_data' will always contain something.
		let global_project_data = self.global_project_data.take().unwrap();
		let project_data = project.load(global_project_data, self.parse_ctx)?;

		restore_terminal();
		let result = project_data.open();

		result.map_err(|err| GenericError::from(err).into())
	}
}
impl TuiData {
	fn buttons(&self) -> impl Iterator<Item = &ProjectButton> {
		self
			.sections
			.iter()
			.flat_map(|section| section.buttons.iter())
	}
}

mod renderer {
	use std::{
		io::{self, Write as _},
		iter,
		ops::Range,
	};

	use crossterm::{
		QueueableCommand as _, cursor,
		style::Color,
		style::{self, StyledContent, Stylize as _},
		terminal,
	};
	use unicode_width::UnicodeWidthStr;

	use super::{Colorscheme, UIState};

	// layouts and renderes the UI state
	// This does *not* draw directly to the terminal. Instead, this returns a
	// 'RenderedContent', which can then be displayed to the terminal. The area for
	// each button is also returned.
	pub fn render(state: &UIState) -> io::Result<(RenderedContent, Vec<Area>)> {
		let tui_data = state.tui_data;
		let mut rendering = RenderedContent {
			lines: Vec::new(),
			terminal_size: terminal::size()?,
		};
		let mut button_areas = Vec::new();
		let mut current_line = 0;

		current_line += rendering.push_centered(
			tui_data.banner.as_str().with(tui_data.colorscheme.banner),
			current_line,
		);
		current_line += 3;

		let max_keybind_width = tui_data
			.buttons()
			.map(|button| button.keybind.width() as u32)
			.max()
			.unwrap_or_default();

		let mut button_idx = 0;
		for section in &tui_data.sections {
			current_line += rendering.push_centered(
				section.heading.as_str().with(tui_data.colorscheme.heading),
				current_line,
			);
			current_line += 1;

			for button in &section.buttons {
				let left_padding =
					(rendering.terminal_size.0 as u32).saturating_sub(tui_data.project_button_width) / 2;
				let trimmed_button_width = tui_data
					.project_button_width
					.min(rendering.terminal_size.0 as u32);

				let button_str = render_project_button(
					&button.keybind,
					&button.project_name,
					trimmed_button_width,
					max_keybind_width,
					button_idx == state.selected_button,
					&tui_data.colorscheme,
				);

				let button_height = rendering.push_str(&button_str, (left_padding, current_line));
				button_areas.push(Area {
					x_range: left_padding..left_padding + trimmed_button_width,
					y_range: current_line..current_line + button_height,
				});
				current_line += button_height;

				button_idx += 1;
			}

			current_line += 2;
		}

		Ok((rendering, button_areas))
	}

	fn render_project_button(
		keybind: &str,
		project_name: &str,
		total_width: u32,
		keybind_width: u32,
		selected: bool,
		colorscheme: &Colorscheme,
	) -> String {
		let box_chars = if selected {
			BOLD_BOX_DRAWING_CHARS
		} else {
			BOX_DRAWING_CHARS
		};

		if total_width == 0 {
			return String::new();
		} else if total_width == 1 {
			return format!(
				"{}\n{}\n{}\n",
				box_chars.down_right, box_chars.vertical, box_chars.up_right
			);
		} else if total_width == 2 {
			return format!(
				"{}{}\n{}{}\n{}{}\n",
				box_chars.down_right,
				box_chars.down_left,
				box_chars.vertical,
				box_chars.vertical,
				box_chars.up_right,
				box_chars.up_left,
			);
		}

		let mut result = String::new();
		let name_box_width = total_width.saturating_sub(keybind_width + 5);
		let keybind_box_width = total_width - name_box_width - 3;

		let top_border = format!(
			"{}{}{}{}{}\n",
			box_chars.down_right,
			iter::repeat_n(box_chars.horizontal, keybind_box_width as usize).collect::<String>(),
			box_chars.horizontal_down,
			iter::repeat_n(box_chars.horizontal, name_box_width as usize).collect::<String>(),
			box_chars.down_left,
		);
		result.push_str(&top_border.with(colorscheme.normal).to_string());

		assert!(
			keybind
				.chars()
				.all(|ch| ch.is_ascii() && !ch.is_ascii_control())
		);
		let left_keybind_padding = (keybind_box_width >= keybind_width + 2) as usize;
		let right_keybind_padding =
			keybind_box_width as i64 - keybind.len() as i64 - left_keybind_padding as i64;
		let keybind_box = format!(
			"{}{}{}",
			" ".repeat(left_keybind_padding),
			keybind[..keybind.len() - (-right_keybind_padding).max(0) as usize].with(colorscheme.keybind),
			" ".repeat(right_keybind_padding.max(0) as usize),
		);

		assert!(
			project_name
				.chars()
				.all(|ch| ch.is_ascii() && !ch.is_ascii_control())
		);
		let left_name_padding = name_box_width.saturating_sub(project_name.len() as u32) / 2;
		let right_name_padding =
			name_box_width as i64 - project_name.len() as i64 - left_name_padding as i64;
		let name_box = format!(
			"{}{}{}",
			" ".repeat(left_name_padding as usize),
			project_name[..project_name.len() - (-right_name_padding).max(0) as usize]
				.with(colorscheme.project_name),
			" ".repeat(right_name_padding.max(0) as usize),
		);

		result.push_str(&format!(
			"{}{keybind_box}{}{name_box}{}\n",
			box_chars.vertical.with(colorscheme.normal),
			box_chars.vertical.with(colorscheme.normal),
			box_chars.vertical.with(colorscheme.normal),
		));

		let bottom_border = format!(
			"{}{}{}{}{}",
			box_chars.up_right,
			iter::repeat_n(box_chars.horizontal, keybind_box_width as usize).collect::<String>(),
			box_chars.horizontal_up,
			iter::repeat_n(box_chars.horizontal, name_box_width as usize).collect::<String>(),
			box_chars.up_left,
		);
		result.push_str(&bottom_border.with(colorscheme.normal).to_string());

		result
	}
	const BOX_DRAWING_CHARS: BoxDrawingChars = BoxDrawingChars {
		vertical: '│',
		horizontal: '─',
		down_left: '┐',
		down_right: '┌',
		up_left: '┘',
		up_right: '└',
		horizontal_down: '┬',
		horizontal_up: '┴',
	};
	const BOLD_BOX_DRAWING_CHARS: BoxDrawingChars = BoxDrawingChars {
		vertical: '┃',
		horizontal: '━',
		down_left: '┓',
		down_right: '┏',
		up_left: '┛',
		up_right: '┗',
		horizontal_down: '┳',
		horizontal_up: '┻',
	};
	struct BoxDrawingChars {
		vertical: char,
		horizontal: char,
		down_left: char,
		down_right: char,
		up_left: char,
		up_right: char,
		horizontal_down: char,
		horizontal_up: char,
	}

	pub struct RenderedContent {
		terminal_size: (u16, u16),
		lines: Vec<((u32, u32), String)>,
	}
	impl RenderedContent {
		fn push_str(&mut self, str: &str, mut pos: (u32, u32)) -> u32 {
			let start_pos_y = pos.1;

			self.lines.extend(str.lines().map(|line| {
				let res = (pos, line.to_owned());
				pos.1 += 1;
				res
			}));

			pos.1 - start_pos_y
		}
		fn push_centered<D: std::fmt::Display>(
			&mut self,
			content: StyledContent<D>,
			pos_y: u32,
		) -> u32 {
			let content_width = content
				.content()
				.to_string()
				.lines()
				.map(UnicodeWidthStr::width)
				.map(|x| x as u32)
				.max()
				.unwrap_or(0);
			let left_padding = ((self.terminal_size.0 as u32).saturating_sub(content_width)) / 2;
			self.push_str(&content.to_string(), (left_padding, pos_y))
		}

		pub fn display(self, background_color: Color) -> io::Result<()> {
			assert!(terminal::is_raw_mode_enabled().is_ok_and(|enabled| enabled));
			let mut stdout = io::stdout();

			stdout
				.queue(style::SetBackgroundColor(background_color))?
				.queue(terminal::Clear(terminal::ClearType::All))?
				.queue(cursor::Hide)?;

			for (pos, line) in self.lines {
				if pos.0 >= self.terminal_size.0 as u32 || pos.1 >= self.terminal_size.1 as u32 {
					continue;
				}

				stdout
					.queue(cursor::MoveTo(pos.0 as u16, pos.1 as u16))?
					.queue(style::Print(line))?;
			}

			stdout.flush()?;
			Ok(())
		}
	}

	pub struct Area {
		x_range: Range<u32>,
		y_range: Range<u32>,
	}
	impl Area {
		pub fn contains(&self, pos: (u16, u16)) -> bool {
			self.x_range.contains(&(pos.0 as u32)) && self.y_range.contains(&(pos.1 as u32))
		}
	}
}
