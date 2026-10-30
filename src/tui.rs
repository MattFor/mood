use std::io::{self, IsTerminal};
use std::time::Duration;

use chrono::{Datelike, NaiveDateTime};
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Clear, Paragraph, Widget, Wrap};
use ratatui::{Frame, Terminal};

use crate::agg::{self, Series, View, fmt_num};
use crate::data;
use crate::entry::Entry;
use crate::kind::{Bucket, ChartKind};
use crate::range::Span;

const PALETTE: [Color; 6] = [
	Color::Cyan,
	Color::Magenta,
	Color::Green,
	Color::Blue,
	Color::LightRed,
	Color::LightBlue,
];

const HELP: &str = "\
↑ ↓          longer / shorter span
← →          move the window
shift ← →    move one day
1 line       2 step       3 scatter    4 bar
5 area       6 heatmap    7 histogram
h            hour of each day
e            exact time of each day, to the minute
t            one line through time, days marked (default)
d w m y u    day, week, month, year, weekday
b            cycle grouping
c            cycle chart
tab          log / averages / reasons
o            time, then highest, then lowest
j k          scroll        PgUp PgDn
home         put today at the end of the window
r            reload
?            close
q            quit

Empty hours are left out, not drawn as zero.
Lines connect the logs you have.
Several logs in one bucket are averaged.
0 is the worst, 10 is the best.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
	Log,
	Averages,
	Reasons,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
	Time,
	High,
	Low,
}

#[derive(Clone, Debug)]
pub struct Ui {
	pub span: Span,
	pub bucket: Bucket,
	pub kind: ChartKind,
	pub panel: Panel,
	pub order: Order,
	pub scroll: usize,
	pub help: bool,
	pub notice: String,
}

impl Ui {
	pub fn new(span: Span, bucket: Bucket, kind: ChartKind) -> Self {
		Self {
			span,
			bucket,
			kind,
			panel: Panel::Log,
			order: Order::Time,
			scroll: 0,
			help: false,
			notice: String::new(),
		}
	}
}

pub fn run(mut entries: Vec<Entry>, mut ui: Ui) -> Result<(), String> {
	if !io::stdout().is_terminal() {
		print_plain(&entries, &ui);
		return Ok(());
	}
	let mut terminal = ratatui::try_init().map_err(|err| err.to_string())?;
	let _ = execute!(io::stdout(), Hide);
	let result = event_loop(&mut terminal, &mut entries, &mut ui);
	let _ = execute!(io::stdout(), Show);
	ratatui::restore();
	result
}

fn event_loop(
	terminal: &mut Terminal<impl ratatui::backend::Backend>,
	entries: &mut Vec<Entry>,
	ui: &mut Ui,
) -> Result<(), String> {
	// Rebuilt per key, not per tick: long exact-time views are heavy.
	let mut view = agg::build_view(entries, ui.span, ui.bucket);
	loop {
		terminal
			.draw(|frame| draw(frame, ui, &view))
			.map_err(|err| err.to_string())?;
		if !event::poll(Duration::from_millis(200)).map_err(|err| err.to_string())? {
			continue;
		}
		match event::read().map_err(|err| err.to_string())? {
			Event::Key(key) if key.kind != KeyEventKind::Release => {
				if on_key(ui, entries, key) == Action::Quit {
					break;
				}
				view = agg::build_view(entries, ui.span, ui.bucket);
			}
			_ => {}
		}
	}
	Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
	Stay,
	Quit,
}

fn on_key(ui: &mut Ui, entries: &mut Vec<Entry>, key: KeyEvent) -> Action {
	if ui.help {
		match key.code {
			KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
			KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
				return Action::Quit;
			}
			_ => ui.help = false,
		}
		return Action::Stay;
	}
	let today = chrono::Local::now().date_naive();
	let fine = key.modifiers.contains(KeyModifiers::SHIFT);
	ui.notice.clear();
	match key.code {
		KeyCode::Char('q') | KeyCode::Esc => return Action::Quit,
		KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Action::Quit,
		KeyCode::Char('?') => ui.help = true,
		KeyCode::Up | KeyCode::Char('+') | KeyCode::Char('=') => {
			let next = ui.span.widen();
			if next == ui.span {
				ui.notice = "already at the longest span".into();
			} else {
				ui.span = next;
				ui.scroll = 0;
			}
		}
		KeyCode::Down | KeyCode::Char('-') => {
			let next = ui.span.shorten();
			if next == ui.span {
				ui.notice = "already one day".into();
			} else {
				ui.span = next;
				ui.scroll = 0;
			}
		}
		KeyCode::Left => {
			ui.span = ui.span.pan(-1, fine, today);
			ui.scroll = 0;
		}
		KeyCode::Right => {
			let next = ui.span.pan(1, fine, today);
			if next == ui.span {
				ui.notice = "already at today".into();
			} else {
				ui.span = next;
				ui.scroll = 0;
			}
		}
		KeyCode::Home | KeyCode::Char('g') => {
			ui.span = ui.span.end_on(today);
			ui.scroll = 0;
		}
		KeyCode::Char(ch) if ch.is_ascii_digit() => {
			if let Some(kind) = ChartKind::from_digit(ch.to_digit(10).unwrap_or(0) as u8) {
				ui.kind = kind;
			}
		}
		KeyCode::Char('c') => ui.kind = ui.kind.cycle(),
		KeyCode::Char('h') => set_bucket(ui, Bucket::HourOfDay),
		KeyCode::Char('e') => set_bucket(ui, Bucket::Exact),
		KeyCode::Char('t') => set_bucket(ui, Bucket::Timeline),
		KeyCode::Char('d') => set_bucket(ui, Bucket::Day),
		KeyCode::Char('w') => set_bucket(ui, Bucket::Week),
		KeyCode::Char('m') => set_bucket(ui, Bucket::Month),
		KeyCode::Char('y') => set_bucket(ui, Bucket::Year),
		KeyCode::Char('u') => set_bucket(ui, Bucket::Weekday),
		KeyCode::Char('b') => set_bucket(ui, ui.bucket.cycle()),
		KeyCode::Tab => {
			ui.panel = match ui.panel {
				Panel::Log => Panel::Averages,
				Panel::Averages => Panel::Reasons,
				Panel::Reasons => Panel::Log,
			};
			ui.scroll = 0;
		}
		KeyCode::Char('o') => {
			ui.order = match ui.order {
				Order::Time => Order::High,
				Order::High => Order::Low,
				Order::Low => Order::Time,
			};
			ui.scroll = 0;
		}
		KeyCode::Char('k') | KeyCode::PageUp => {
			ui.scroll = ui
				.scroll
				.saturating_add(if key.code == KeyCode::PageUp { 8 } else { 1 })
		}
		KeyCode::Char('j') | KeyCode::PageDown => {
			ui.scroll = ui
				.scroll
				.saturating_sub(if key.code == KeyCode::PageDown { 8 } else { 1 })
		}
		KeyCode::Char('r') => match data::load() {
			Ok(fresh) => {
				let count = fresh.len();
				*entries = fresh;
				let _ = data::write_summary(entries);
				ui.notice = format!("reloaded {count}");
			}
			Err(err) => ui.notice = err,
		},
		_ => {}
	}
	Action::Stay
}

fn set_bucket(ui: &mut Ui, bucket: Bucket) {
	ui.bucket = bucket;
	ui.scroll = 0;
}

fn print_plain(entries: &[Entry], ui: &Ui) {
	let view = agg::build_view(entries, ui.span, ui.bucket);
	println!("{}", agg::subtitle(&view.summary, ui.span));
	println!("{} · {}", ui.kind.label(), ui.bucket.label());
	if view.rows.is_empty() {
		println!("no logs in this range");
		return;
	}
	for row in &view.rows {
		println!(
			"{}  mean {}  min {}  max {}  n {}",
			row.label,
			fmt_num(row.mean),
			fmt_num(row.min),
			fmt_num(row.max),
			row.n
		);
	}
}

fn draw(frame: &mut Frame, ui: &Ui, view: &View) {
	let area = frame.area();
	let [header, chart, bottom, footer] = Layout::vertical([
		Constraint::Length(2),
		Constraint::Min(6),
		Constraint::Length(8),
		Constraint::Length(2),
	])
	.areas(area);
	let buf = frame.buffer_mut();
	draw_header(buf, header, ui, view);
	draw_chart(buf, chart, ui, view);
	draw_bottom(buf, bottom, ui, view);
	draw_footer(buf, footer);
	if ui.help {
		draw_help(frame);
	}
}

fn draw_header(buf: &mut ratatui::buffer::Buffer, area: Rect, ui: &Ui, view: &View) {
	let mut title = format!(
		"mood  {}  ·  {}  ·  {}",
		ui.span.label(),
		ui.kind.label(),
		ui.bucket.label()
	);
	if !ui.notice.is_empty() {
		title.push_str("   ");
		title.push_str(&ui.notice);
	}
	put(buf, area.x, area.y, &title, Style::new().add_modifier(Modifier::BOLD));
	if area.height < 2 {
		return;
	}
	if view.summary.n == 0 {
		put(
			buf,
			area.x,
			area.y + 1,
			"no logs in this range",
			Style::new().fg(Color::DarkGray),
		);
		return;
	}
	let prefix = "mean ";
	put(buf, area.x, area.y + 1, prefix, Style::new());
	let mean = fmt_num(view.summary.mean);
	let mean_x = area.x + prefix.len() as u16;
	put(
		buf,
		mean_x,
		area.y + 1,
		&mean,
		Style::new()
			.fg(mood_color(view.summary.mean))
			.add_modifier(Modifier::BOLD),
	);
	let rest = format!(
		"   median {}   min {}   max {}   sd {}   n {}",
		fmt_num(view.summary.median),
		fmt_num(view.summary.min),
		fmt_num(view.summary.max),
		fmt_num(view.summary.sd),
		view.summary.n
	);
	put(buf, mean_x + mean.len() as u16, area.y + 1, &rest, Style::new());
}

fn draw_footer(buf: &mut ratatui::buffer::Buffer, area: Rect) {
	let style = Style::new().fg(Color::DarkGray);
	put(
		buf,
		area.x,
		area.y,
		"↑↓ span   ←→ move   shift←→ one day   1-7 chart   h e t d w m y u group   b c cycle",
		style,
	);
	if area.height > 1 {
		put(
			buf,
			area.x,
			area.y + 1,
			"tab log/averages/reasons   o sort   j k scroll   home today   r reload   ? help   q quit",
			style,
		);
	}
}

fn draw_help(frame: &mut Frame) {
	let area = frame.area();
	let width = area.width.min(72);
	let height = area.height.min(26);
	if width < 24 || height < 8 {
		return;
	}
	let popup = Rect {
		x: area.x + (area.width - width) / 2,
		y: area.y + (area.height - height) / 2,
		width,
		height,
	};
	let paragraph = Paragraph::new(HELP)
		.wrap(Wrap { trim: false })
		.block(Block::bordered().title(" keys "));
	frame.render_widget(Clear, popup);
	frame.render_widget(paragraph, popup);
}

fn draw_chart(buf: &mut ratatui::buffer::Buffer, area: Rect, ui: &Ui, view: &View) {
	let rows_fit = area.height.saturating_sub(3) as usize;
	let title = chart_title(ui.kind, ui.bucket, view, rows_fit);
	let block = Block::bordered().title(format!(" {title} "));
	let inner = block.inner(area);
	Widget::render(block, area, buf);
	if inner.width < 8 || inner.height < 3 {
		put(buf, inner.x, inner.y, "too small", Style::new().fg(Color::DarkGray));
		return;
	}
	if ui.kind == ChartKind::Heatmap {
		draw_heatmap(buf, inner, view);
		return;
	}
	let axis = axis_for(ui.kind, view);
	let gutter = [axis.top.len(), axis.mid.len(), axis.bottom.len()]
		.into_iter()
		.max()
		.unwrap_or(1) as u16
		+ 1;
	if inner.width <= gutter + 2 {
		return;
	}
	let plot = Rect {
		x: inner.x + gutter,
		y: inner.y,
		width: inner.width - gutter,
		height: inner.height - 1,
	};
	let label_y = inner.bottom().saturating_sub(1);
	put_right(buf, plot.x, plot.y, &axis.top, Style::new().fg(Color::DarkGray));
	if plot.height > 3 {
		put_right(
			buf,
			plot.x,
			plot.y + plot.height / 2,
			&axis.mid,
			Style::new().fg(Color::DarkGray),
		);
	}
	put_right(
		buf,
		plot.x,
		plot.y + plot.height.saturating_sub(1),
		&axis.bottom,
		Style::new().fg(Color::DarkGray),
	);
	match ui.kind {
		ChartKind::Bar => {
			let series = bar_series(&view.series);
			let values = series.map(|series| series.y.clone()).unwrap_or_default();
			draw_blocks(buf, plot, &values, &axis, BarMode::Value);
			label_categories(buf, plot, label_y, &view.categories, ui.bucket, ui.span, false);
		}
		ChartKind::Histogram => {
			let values: Vec<Option<f64>> = view.hist.iter().map(|count| Some(*count as f64)).collect();
			let cats: Vec<String> = (0..11).map(|bin| bin.to_string()).collect();
			draw_blocks(buf, plot, &values, &axis, BarMode::Index);
			label_categories(buf, plot, label_y, &cats, ui.bucket, ui.span, false);
		}
		_ => {
			let shown = series_to_draw(&view.series);
			let days = day_starts(ui, view, plot.width);
			draw_braille(
				buf,
				plot,
				&shown,
				&axis,
				ui.kind,
				days.as_deref().unwrap_or_default(),
				view.categories.len(),
			);
			match days {
				Some(days) => label_days(buf, plot, label_y, &days, view.categories.len(), ui.span),
				None => label_categories(buf, plot, label_y, &view.categories, ui.bucket, ui.span, true),
			}
		}
	}
	if view.summary.n == 0 {
		let message = "no logs in this range";
		let x = plot.x + plot.width.saturating_sub(message.len() as u16) / 2;
		put(
			buf,
			x,
			plot.y + plot.height / 2,
			message,
			Style::new().fg(Color::DarkGray),
		);
	}
}

fn chart_title(kind: ChartKind, bucket: Bucket, view: &View, rows_fit: usize) -> String {
	match kind {
		ChartKind::Heatmap if view.heatmap.len() > rows_fit && rows_fit > 0 => {
			format!("heatmap · latest {rows_fit} of {} days", view.heatmap.len())
		}
		ChartKind::Heatmap => "heatmap · day × hour".into(),
		ChartKind::Histogram => "histogram · how often each score".into(),
		ChartKind::Bar => {
			let name = bar_series(&view.series)
				.map(|series| series.name.as_str())
				.unwrap_or("mood");
			format!("bar · {name}")
		}
		_ if view.series.len() > 8 => {
			let tail = if view.series.iter().any(|series| series.emphasize) {
				"latest days and the mean"
			} else {
				"latest days"
			};
			format!("{} · {} · {tail}", kind.label(), bucket.label())
		}
		_ => {
			let names: Vec<&str> = series_to_draw(&view.series)
				.iter()
				.map(|series| series.name.as_str())
				.collect();
			if names.is_empty() {
				format!("{} · {}", kind.label(), bucket.label())
			} else {
				format!("{} · {}", kind.label(), names.join("  "))
			}
		}
	}
}

fn draw_bottom(buf: &mut ratatui::buffer::Buffer, area: Rect, ui: &Ui, view: &View) {
	let title = match ui.panel {
		Panel::Log => match ui.order {
			Order::Time => format!(" log · {} ", view.logs.len()),
			Order::High => format!(" log · highest at the bottom · {} ", view.logs.len()),
			Order::Low => format!(" log · lowest at the bottom · {} ", view.logs.len()),
		},
		Panel::Averages => format!(" averages · {} ", view.rows.len()),
		Panel::Reasons => format!(" reasons · {} · mean mood when you gave each one ", view.reasons.len()),
	};
	let block = Block::bordered().title(title);
	let inner = block.inner(area);
	Widget::render(block, area, buf);
	if inner.height == 0 {
		return;
	}
	match ui.panel {
		Panel::Log => draw_logs(buf, inner, ui, view),
		Panel::Averages => draw_rows(buf, inner, ui, view),
		Panel::Reasons => draw_reasons(buf, inner, ui, view),
	}
}

fn draw_logs(buf: &mut ratatui::buffer::Buffer, area: Rect, ui: &Ui, view: &View) {
	if view.logs.is_empty() {
		put(
			buf,
			area.x,
			area.y,
			"no logs in this range",
			Style::new().fg(Color::DarkGray),
		);
		return;
	}
	let multi_host = view
		.logs
		.iter()
		.map(|entry| entry.host.as_str())
		.collect::<std::collections::HashSet<_>>()
		.len() > 1;
	let mut items = ordered(&view.logs, ui.order);
	let rows = area.height as usize;
	let max_scroll = items.len().saturating_sub(rows);
	let scroll = ui.scroll.min(max_scroll);
	let end = items.len() - scroll;
	let start = end.saturating_sub(rows);
	for (offset, entry) in items.drain(start..end).enumerate() {
		let y = area.y + offset as u16;
		let when = log_when(entry, ui.span);
		let score = fmt_num(entry.mood);
		put(buf, area.x, y, &when, Style::new().fg(Color::DarkGray));
		let score_x = area.x + when.chars().count() as u16 + 1;
		put(
			buf,
			score_x,
			y,
			&score,
			Style::new().fg(mood_color(entry.mood)).add_modifier(Modifier::BOLD),
		);
		let mut note = entry.reasons.join(", ");
		if multi_host && !entry.host.is_empty() {
			if !note.is_empty() {
				note.push_str(" · ");
			}
			note.push_str(&entry.host);
		}
		let note_x = score_x + score.chars().count() as u16 + 1;
		put(buf, note_x, y, &note, Style::new());
	}
}

fn ordered(logs: &[Entry], order: Order) -> Vec<&Entry> {
	let mut items: Vec<&Entry> = logs.iter().collect();
	match order {
		Order::Time => items.sort_by(|a, b| a.at.cmp(&b.at).then(a.host.cmp(&b.host))),
		Order::High => items.sort_by(|a, b| {
			a.mood
				.partial_cmp(&b.mood)
				.unwrap_or(std::cmp::Ordering::Equal)
				.then(a.at.cmp(&b.at))
		}),
		Order::Low => items.sort_by(|a, b| {
			b.mood
				.partial_cmp(&a.mood)
				.unwrap_or(std::cmp::Ordering::Equal)
				.then(a.at.cmp(&b.at))
		}),
	}
	items
}

fn log_when(entry: &Entry, span: Span) -> String {
	if span.start == span.end {
		entry.at.format("%H:%M").to_string()
	} else if span.start.year() != span.end.year() {
		entry.at.format("%Y-%m-%d %H:%M").to_string()
	} else {
		entry.at.format("%m-%d %H:%M").to_string()
	}
}

fn draw_rows(buf: &mut ratatui::buffer::Buffer, area: Rect, ui: &Ui, view: &View) {
	if view.rows.is_empty() {
		put(
			buf,
			area.x,
			area.y,
			"no logs in this range",
			Style::new().fg(Color::DarkGray),
		);
		return;
	}
	let header = format!("{:<12} {:>5} {:>5} {:>5} {:>4}", "bucket", "mean", "min", "max", "n");
	put(buf, area.x, area.y, &header, Style::new().fg(Color::DarkGray));
	let rows = area.height.saturating_sub(1) as usize;
	let max_scroll = view.rows.len().saturating_sub(rows);
	let scroll = ui.scroll.min(max_scroll);
	let end = view.rows.len() - scroll;
	let start = end.saturating_sub(rows);
	for (offset, row) in view.rows[start..end].iter().enumerate() {
		let label = clip(&row.label, 12);
		let line = format!(
			"{label:<12} {:>5} {:>5} {:>5} {:>4}",
			fmt_num(row.mean),
			fmt_num(row.min),
			fmt_num(row.max),
			row.n
		);
		put(
			buf,
			area.x,
			area.y + 1 + offset as u16,
			&line,
			Style::new().fg(mood_color(row.mean)),
		);
	}
}

fn draw_reasons(buf: &mut ratatui::buffer::Buffer, area: Rect, ui: &Ui, view: &View) {
	if view.reasons.is_empty() {
		put(
			buf,
			area.x,
			area.y,
			"no reasons in this range. log one with: mood 4 work, bad sleep",
			Style::new().fg(Color::DarkGray),
		);
		return;
	}
	let header = format!("{:<24} {:>5} {:>6} {:>4}", "reason", "mean", "vs avg", "n");
	put(buf, area.x, area.y, &header, Style::new().fg(Color::DarkGray));
	let rows = area.height.saturating_sub(1) as usize;
	let scroll = ui.scroll.min(view.reasons.len().saturating_sub(rows));
	for (offset, row) in view.reasons.iter().skip(scroll).take(rows).enumerate() {
		let delta = row.mean - view.summary.mean;
		let sign = if delta < -0.05 { "-" } else { "+" };
		let line = format!(
			"{:<24} {:>5} {:>6} {:>4}",
			clip(&row.reason, 24),
			fmt_num(row.mean),
			format!("{sign}{}", fmt_num(delta.abs())),
			row.n
		);
		put(
			buf,
			area.x,
			area.y + 1 + offset as u16,
			&line,
			Style::new().fg(mood_color(row.mean)),
		);
	}
}

struct Axis {
	min: f64,
	max: f64,
	top: String,
	mid: String,
	bottom: String,
}

fn axis_for(kind: ChartKind, view: &View) -> Axis {
	if kind == ChartKind::Histogram {
		let max = view.hist.iter().copied().max().unwrap_or(0).max(1) as f64;
		Axis {
			min: 0.0,
			max,
			top: fmt_num(max),
			mid: fmt_num(max / 2.0),
			bottom: "0".into(),
		}
	} else {
		Axis {
			min: 0.0,
			max: 10.0,
			top: "10".into(),
			mid: "5".into(),
			bottom: "0".into(),
		}
	}
}

fn series_to_draw(series: &[Series]) -> Vec<&Series> {
	if series.len() <= 8 {
		return series.iter().collect();
	}
	let mut picked: Vec<&Series> = series.iter().filter(|series| !series.emphasize).rev().take(6).collect();
	picked.reverse();
	if let Some(mean) = series.iter().find(|series| series.emphasize) {
		picked.push(mean);
	}
	picked
}

fn bar_series(series: &[Series]) -> Option<&Series> {
	if series.len() <= 1 {
		series.first()
	} else {
		series.iter().find(|series| series.emphasize).or(series.last())
	}
}

#[derive(Clone, Copy)]
enum BarMode {
	Value,
	Index,
}

fn draw_blocks(buf: &mut ratatui::buffer::Buffer, plot: Rect, values: &[Option<f64>], axis: &Axis, mode: BarMode) {
	let squeezed;
	let values = if matches!(mode, BarMode::Value) && values.len() > plot.width as usize {
		squeezed = squeeze(values, plot.width as usize);
		&squeezed[..]
	} else {
		values
	};
	let count = values.len();
	if count == 0 || plot.width < count as u16 || plot.height == 0 {
		put(
			buf,
			plot.x,
			plot.y,
			"too many bars for this width",
			Style::new().fg(Color::DarkGray),
		);
		return;
	}
	let slot = plot.width / count as u16;
	let gap = u16::from(slot >= 3);
	let bar_w = (slot - gap).max(1);
	let halves = plot.height as usize * 2;
	let span = (axis.max - axis.min).abs().max(f64::EPSILON);
	for (index, value) in values.iter().enumerate() {
		let Some(value) = value else { continue };
		let tone = ((*value - axis.min) / span).clamp(0.0, 1.0);
		let mut filled = (tone * halves as f64).round() as usize;
		if filled == 0 {
			if matches!(mode, BarMode::Value) {
				filled = 1;
			} else {
				continue;
			}
		}
		let color = match mode {
			BarMode::Value => mood_color(*value),
			BarMode::Index => mood_color(index as f64),
		};
		let x0 = plot.x + index as u16 * slot + (slot - bar_w) / 2;
		for column in 0..bar_w {
			let x = x0 + column;
			let mut remain = filled;
			for row in (0..plot.height).rev() {
				if remain == 0 {
					break;
				}
				let y = plot.y + row;
				if x >= buf.area.right() || y >= buf.area.bottom() {
					continue;
				}
				if remain >= 2 {
					buf[(x, y)].set_symbol("█").set_fg(color);
					remain -= 2;
				} else {
					buf[(x, y)].set_symbol("▄").set_fg(color);
					remain = 0;
				}
			}
		}
	}
}

/// More slots than columns: average the slots sharing one.
fn squeeze(values: &[Option<f64>], width: usize) -> Vec<Option<f64>> {
	let mut columns = vec![Vec::new(); width];
	for (index, value) in values.iter().enumerate() {
		if let (Some(value), Some(column)) = (value, columns.get_mut(index * width / values.len())) {
			column.push(*value);
		}
	}
	columns.iter().map(|column| agg::mean_opt(column)).collect()
}

fn draw_heatmap(buf: &mut ratatui::buffer::Buffer, area: Rect, view: &View) {
	let gutter = 12u16;
	if area.width <= gutter + 8 || area.height < 2 {
		put(buf, area.x, area.y, "too small", Style::new().fg(Color::DarkGray));
		return;
	}
	let cell_w = ((area.width - gutter) / 24).max(1);
	for hour in [0u16, 6, 12, 18] {
		put(
			buf,
			area.x + gutter + hour * cell_w,
			area.y,
			&format!("{hour:02}"),
			Style::new().fg(Color::DarkGray),
		);
	}
	let rows_fit = area.height.saturating_sub(1) as usize;
	let total = view.heatmap.len();
	let start = total.saturating_sub(rows_fit);
	for (offset, row_index) in (start..total).enumerate() {
		let y = area.y + 1 + offset as u16;
		let label = view.heatmap_rows.get(row_index).map(String::as_str).unwrap_or("");
		put(
			buf,
			area.x,
			y,
			&clip(label, (gutter as usize).saturating_sub(1)),
			Style::new().fg(Color::DarkGray),
		);
		for hour in 0..24 {
			let value = view
				.heatmap
				.get(row_index)
				.and_then(|row| row.get(hour))
				.copied()
				.flatten();
			let (symbol, color) = match value {
				Some(mood) => ("█", mood_color(mood)),
				None => ("·", Color::Rgb(70, 70, 70)),
			};
			let x0 = area.x + gutter + hour as u16 * cell_w;
			for dx in 0..cell_w {
				let x = x0 + dx;
				if x < buf.area.right() && y < buf.area.bottom() {
					buf[(x, y)].set_symbol(symbol).set_fg(color);
				}
			}
		}
	}
}

fn label_categories(
	buf: &mut ratatui::buffer::Buffer,
	plot: Rect,
	y: u16,
	categories: &[String],
	bucket: Bucket,
	span: Span,
	braille: bool,
) {
	let indexes = label_indexes(categories.len());
	let mut labels = Vec::new();
	for index in indexes {
		let text = axis_label(&categories[index], bucket, span);
		let center = if braille {
			plot.x + (map_x(index, categories.len(), plot.width as usize * 2) as u16) / 2
		} else if categories.len() > plot.width as usize {
			// Same column squeeze() puts the bar in.
			plot.x + (index * plot.width as usize / categories.len()) as u16
		} else {
			let count = categories.len() as u16;
			let slot = plot.width.checked_div(count).unwrap_or(1);
			plot.x + index as u16 * slot + slot / 2
		};
		labels.push((center, text));
	}
	let borrowed: Vec<(u16, &str)> = labels.iter().map(|(center, text)| (*center, text.as_str())).collect();
	draw_x_labels(buf, y, plot.right(), &borrowed);
}

/// Only when every day is wide enough for its name.
fn day_starts(ui: &Ui, view: &View, width: u16) -> Option<Vec<usize>> {
	let days = ui.span.len_days() as usize;
	if ui.bucket != Bucket::Timeline || days < 2 || (width as usize) < days * 7 {
		return None;
	}
	// Timeline steps divide a day, so days have equal slot counts.
	let per_day = view.categories.len() / days;
	Some((0..days).map(|day| day * per_day).collect())
}

fn label_days(buf: &mut ratatui::buffer::Buffer, plot: Rect, y: u16, days: &[usize], count: usize, span: Span) {
	let per_day = count / days.len();
	let labels: Vec<(u16, String)> = days
		.iter()
		.zip(span.dates())
		.map(|(start, date)| {
			let center = plot.x + (map_x(start + per_day / 2, count, plot.width as usize * 2) as u16) / 2;
			(center, date.format("%a %d").to_string())
		})
		.collect();
	let borrowed: Vec<(u16, &str)> = labels.iter().map(|(center, text)| (*center, text.as_str())).collect();
	draw_x_labels(buf, y, plot.right(), &borrowed);
}

fn label_indexes(count: usize) -> Vec<usize> {
	if count == 0 {
		return Vec::new();
	}
	if count <= 8 {
		return (0..count).collect();
	}
	let mut indexes = vec![0];
	for step in 1..4 {
		indexes.push(step * count / 4);
	}
	indexes.push(count - 1);
	indexes.dedup();
	indexes
}

fn axis_label(category: &str, bucket: Bucket, span: Span) -> String {
	if bucket == Bucket::Timeline {
		if let Ok(stamp) = NaiveDateTime::parse_from_str(category, "%Y-%m-%dT%H:%M:%S") {
			return if span.len_days() <= 1 {
				stamp.format("%H:%M").to_string()
			} else if span.len_days() <= 14 {
				stamp.format("%d %H:%M").to_string()
			} else {
				stamp.format("%m-%d").to_string()
			};
		}
	}
	category.to_string()
}

fn draw_x_labels(buf: &mut ratatui::buffer::Buffer, y: u16, right: u16, labels: &[(u16, &str)]) {
	let mut next_free = 0u16;
	for (center, text) in labels {
		let width = text.chars().count() as u16;
		let start = center.saturating_sub(width / 2).min(right.saturating_sub(width));
		if start < next_free {
			continue;
		}
		put(buf, start, y, text, Style::new().fg(Color::DarkGray));
		next_free = start + width + 1;
	}
}

struct Dots {
	w: usize,
	h: usize,
	px: Vec<u8>,
}

impl Dots {
	fn new(w: usize, h: usize) -> Self {
		let len = w.saturating_mul(h);
		Self { w, h, px: vec![0; len] }
	}

	fn plot(&mut self, x: isize, y: isize, color: u8) {
		if color == 0 || x < 0 || y < 0 {
			return;
		}
		let (x, y) = (x as usize, y as usize);
		if x >= self.w || y >= self.h || self.w == 0 {
			return;
		}
		self.px[y * self.w + x] = color;
	}

	fn blit(&self, buf: &mut ratatui::buffer::Buffer, plot: Rect, palette: &[Color]) {
		for cy in 0..plot.height as usize {
			for cx in 0..plot.width as usize {
				let mut data_bits = 0u8;
				let mut ref_bits = 0u8;
				let mut data_color = None;
				for dy in 0..4 {
					for dx in 0..2 {
						let x = cx * 2 + dx;
						let y = cy * 4 + dy;
						if x >= self.w || y >= self.h {
							continue;
						}
						let id = self.px[y * self.w + x];
						if id == 0 {
							continue;
						}
						let bit = braille_bit(dx, dy);
						if id == 1 {
							ref_bits |= bit;
						} else {
							data_bits |= bit;
							data_color = Some(id);
						}
					}
				}
				let (bits, color) = if let Some(id) = data_color {
					(data_bits, palette.get(id as usize).copied().unwrap_or(Color::White))
				} else if ref_bits != 0 {
					(ref_bits, Color::DarkGray)
				} else {
					continue;
				};
				let Some(ch) = char::from_u32(0x2800 + bits as u32) else {
					continue;
				};
				let x = plot.x + cx as u16;
				let y = plot.y + cy as u16;
				if x < buf.area.right() && y < buf.area.bottom() {
					buf[(x, y)].set_char(ch).set_fg(color);
				}
			}
		}
	}
}

fn draw_braille(
	buf: &mut ratatui::buffer::Buffer,
	plot: Rect,
	series: &[&Series],
	axis: &Axis,
	kind: ChartKind,
	midnights: &[usize],
	count: usize,
) {
	if plot.width == 0 || plot.height == 0 {
		return;
	}
	let mut dots = Dots::new(plot.width as usize * 2, plot.height as usize * 4);
	let reference = if kind == ChartKind::Histogram {
		(axis.min + axis.max) / 2.0
	} else {
		5.0
	};
	let y_ref = map_y(reference, axis, dots.h);
	for x in (0..dots.w).step_by(2) {
		dots.plot(x as isize, y_ref, 1);
	}
	for &slot in midnights.iter().skip(1) {
		let x = map_x(slot, count, dots.w);
		for y in (0..dots.h).step_by(2) {
			dots.plot(x, y as isize, 1);
		}
	}
	let mut palette = vec![Color::Reset, Color::DarkGray];
	let mut ordered: Vec<&Series> = series.iter().copied().filter(|series| series.emphasize).collect();
	ordered.extend(series.iter().copied().filter(|series| !series.emphasize));
	for (index, series) in ordered.iter().enumerate() {
		let color = if series.emphasize {
			Color::Yellow
		} else {
			PALETTE[index % PALETTE.len()]
		};
		let id = palette.len() as u8;
		palette.push(color);
		draw_series(&mut dots, series, axis, kind, id);
	}
	dots.blit(buf, plot, &palette);
}

fn draw_series(dots: &mut Dots, series: &Series, axis: &Axis, kind: ChartKind, color: u8) {
	let count = series.y.len();
	let xy: Vec<(isize, isize)> = series
		.y
		.iter()
		.enumerate()
		.filter_map(|(index, value)| value.map(|value| (map_x(index, count, dots.w), map_y(value, axis, dots.h))))
		.collect();
	if xy.is_empty() {
		return;
	}
	if kind == ChartKind::Scatter || xy.len() == 1 {
		for (x, y) in &xy {
			mark(dots, *x, *y, color);
		}
		if kind == ChartKind::Scatter || xy.len() == 1 {
			return;
		}
	}
	let poly = if kind == ChartKind::Step { step_poly(&xy) } else { xy };
	if kind == ChartKind::Area {
		let base = map_y(axis.min, axis, dots.h);
		for pair in poly.windows(2) {
			fill_under(dots, pair[0].0, pair[0].1, pair[1].0, pair[1].1, base, color);
		}
	}
	for pair in poly.windows(2) {
		stroke(dots, pair[0].0, pair[0].1, pair[1].0, pair[1].1, color);
	}
}

fn step_poly(points: &[(isize, isize)]) -> Vec<(isize, isize)> {
	let mut poly = vec![points[0]];
	for &(x, y) in points.iter().skip(1) {
		let (_, previous_y) = *poly.last().unwrap();
		poly.push((x, previous_y));
		poly.push((x, y));
	}
	poly
}

fn mark(dots: &mut Dots, x: isize, y: isize, color: u8) {
	dots.plot(x, y, color);
	dots.plot(x - 1, y, color);
	dots.plot(x + 1, y, color);
	dots.plot(x, y - 1, color);
	dots.plot(x, y + 1, color);
}

fn fill_under(dots: &mut Dots, x0: isize, y0: isize, x1: isize, y1: isize, base: isize, color: u8) {
	let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
	for step in 0..=steps {
		let t = step as f64 / steps as f64;
		let x = (x0 as f64 + (x1 - x0) as f64 * t).round() as isize;
		let y = (y0 as f64 + (y1 - y0) as f64 * t).round() as isize;
		fill_column(dots, x, y, base, color);
	}
}

fn fill_column(dots: &mut Dots, x: isize, y: isize, base: isize, color: u8) {
	let (top, bottom) = if y <= base { (y, base) } else { (base, y) };
	for yy in top..=bottom {
		dots.plot(x, yy, color);
	}
}

fn stroke(dots: &mut Dots, mut x0: isize, mut y0: isize, x1: isize, y1: isize, color: u8) {
	let dx = (x1 - x0).abs();
	let sx = if x0 < x1 { 1 } else { -1 };
	let dy = -(y1 - y0).abs();
	let sy = if y0 < y1 { 1 } else { -1 };
	let mut err = dx + dy;
	let mut guard = dx + dy.abs() + 4;
	loop {
		dots.plot(x0, y0, color);
		if (x0 == x1 && y0 == y1) || guard == 0 {
			break;
		}
		guard -= 1;
		let doubled = 2 * err;
		if doubled >= dy {
			err += dy;
			x0 += sx;
		}
		if doubled <= dx {
			err += dx;
			y0 += sy;
		}
	}
}

fn map_x(index: usize, count: usize, width: usize) -> isize {
	if width == 0 {
		return 0;
	}
	if count <= 1 {
		return (width as isize) / 2;
	}
	((index as f64) * (width as f64 - 1.0) / (count as f64 - 1.0)).round() as isize
}

fn map_y(value: f64, axis: &Axis, height: usize) -> isize {
	if height == 0 {
		return 0;
	}
	let span = axis.max - axis.min;
	let t = if span.abs() < f64::EPSILON {
		0.5
	} else {
		((value - axis.min) / span).clamp(0.0, 1.0)
	};
	((1.0 - t) * (height as f64 - 1.0)).round() as isize
}

fn braille_bit(dx: usize, dy: usize) -> u8 {
	match (dx, dy) {
		(0, 0) => 0x01,
		(0, 1) => 0x02,
		(0, 2) => 0x04,
		(0, 3) => 0x40,
		(1, 0) => 0x08,
		(1, 1) => 0x10,
		(1, 2) => 0x20,
		(1, 3) => 0x80,
		_ => 0,
	}
}

fn mood_color(value: f64) -> Color {
	let t = (value / 10.0).clamp(0.0, 1.0);
	let (red, green) = if t < 0.5 {
		let u = t / 0.5;
		(210, (50.0 + 150.0 * u) as u8)
	} else {
		let u = (t - 0.5) / 0.5;
		((210.0 * (1.0 - u)) as u8, 190)
	};
	Color::Rgb(red, green, 70)
}

fn put(buf: &mut ratatui::buffer::Buffer, x: u16, y: u16, text: &str, style: Style) {
	if y < buf.area.y || y >= buf.area.bottom() || x >= buf.area.right() {
		return;
	}
	buf.set_stringn(x, y, text, buf.area.right().saturating_sub(x) as usize, style);
}

fn put_right(buf: &mut ratatui::buffer::Buffer, right: u16, y: u16, text: &str, style: Style) {
	let width = text.chars().count() as u16;
	put(buf, right.saturating_sub(width), y, text, style);
}

fn clip(text: &str, width: usize) -> String {
	text.chars().take(width).collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use chrono::NaiveDate;
	use ratatui::backend::TestBackend;

	fn sample() -> Entry {
		Entry {
			at: NaiveDateTime::parse_from_str("2026-10-20 14:05", "%Y-%m-%d %H:%M").unwrap(),
			t: "2026-10-20T14:05:00+02:00".into(),
			mood: 6.0,
			reasons: vec!["ok".into()],
			host: "test".into(),
		}
	}

	fn frame_text(ui: &Ui, view: &View) -> String {
		let backend = TestBackend::new(100, 32);
		let mut terminal = Terminal::new(backend).unwrap();
		terminal.draw(|frame| draw(frame, ui, view)).unwrap();
		terminal
			.backend()
			.buffer()
			.content
			.iter()
			.map(|cell| cell.symbol())
			.collect()
	}

	#[test]
	fn draws_every_chart_and_the_log() {
		let entry = sample();
		let span = Span::day(entry.at.date());
		let view = agg::build_view(std::slice::from_ref(&entry), span, Bucket::HourOfDay);
		for kind in [
			ChartKind::Line,
			ChartKind::Step,
			ChartKind::Scatter,
			ChartKind::Bar,
			ChartKind::Area,
			ChartKind::Heatmap,
			ChartKind::Histogram,
		] {
			let ui = Ui::new(span, Bucket::HourOfDay, kind);
			let text = frame_text(&ui, &view);
			assert!(text.contains("14:05"), "{kind:?} missing log\n{text}");
			assert!(text.contains("mean"), "{kind:?} missing mean");
		}
		let mut ui = Ui::new(span, Bucket::HourOfDay, ChartKind::Line);
		ui.panel = Panel::Reasons;
		let text = frame_text(&ui, &view);
		assert!(text.contains("reasons · 1"), "missing reasons panel\n{text}");
	}

	#[test]
	fn exact_bars_fit_the_width() {
		let entry = sample();
		let span = Span::day(entry.at.date());
		for bucket in [Bucket::Exact, Bucket::Timeline] {
			let view = agg::build_view(std::slice::from_ref(&entry), span, bucket);
			for kind in [ChartKind::Line, ChartKind::Bar, ChartKind::Scatter] {
				let text = frame_text(&Ui::new(span, bucket, kind), &view);
				assert!(!text.contains("too many bars"), "{bucket:?} {kind:?}\n{text}");
				assert!(text.contains("06:00"), "{bucket:?} {kind:?} axis\n{text}");
			}
		}
		assert_eq!(
			squeeze(&[Some(2.0), Some(4.0), None, Some(9.0)], 2),
			[Some(3.0), Some(9.0)]
		);
	}

	#[test]
	fn week_timeline_names_each_day() {
		let entry = sample();
		let span = Span::ending(entry.at.date(), 7);
		let view = agg::build_view(std::slice::from_ref(&entry), span, Bucket::Timeline);
		let text = frame_text(&Ui::new(span, Bucket::Timeline, ChartKind::Line), &view);
		for day in ["Wed 14", "Sat 17", "Tue 20"] {
			assert!(text.contains(day), "missing {day}\n{text}");
		}
	}

	#[test]
	fn empty_range_says_so() {
		let span = Span::day(NaiveDate::from_ymd_opt(2026, 10, 20).unwrap());
		let ui = Ui::new(span, Bucket::HourOfDay, ChartKind::Line);
		let view = agg::build_view(&[], span, Bucket::HourOfDay);
		let text = frame_text(&ui, &view);
		assert!(text.contains("no logs"), "{text}");
	}
}
