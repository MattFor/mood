use std::io::{BufRead, Write};

use chrono::{DateTime, Datelike, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

use crate::entry::split_reasons;
use crate::kind::{Bucket, ChartKind};
use crate::plot;
use crate::range::{self, Span};

#[derive(Clone, Debug)]
pub enum Command {
	Help,
	Version,
	Status,
	Doctor,
	Path,
	Undo,
	Log {
		mood: f64,
		reasons: Vec<String>,
		at: DateTime<Local>,
	},
	Reasons {
		span: Span,
	},
	Stats {
		span: Span,
		bucket: Bucket,
		kind: ChartKind,
	},
	Graph {
		span: Span,
		bucket: Bucket,
		kind: ChartKind,
		out: Option<std::path::PathBuf>,
	},
}

pub fn parse(args: &[String], now: DateTime<Local>) -> Result<Command, String> {
	if args.is_empty() {
		return Ok(Command::Status);
	}
	match args[0].as_str() {
		"help" | "--help" | "-h" => Ok(Command::Help),
		"version" | "--version" | "-V" => Ok(Command::Version),
		"stats" | "s" if has_help(&args[1..]) => Ok(Command::Help),
		"stats" | "s" => parse_stats(&args[1..], now.date_naive()),
		"graph" if has_help(&args[1..]) => Ok(Command::Help),
		"graph" => parse_graph(&args[1..], now.date_naive()),
		"reasons" | "why" if has_help(&args[1..]) => Ok(Command::Help),
		"reasons" | "why" => {
			let today = now.date_naive();
			let span = if args.len() == 1 {
				Span::ending(today, 30)
			} else {
				range::parse_when(&args[1..].join(" "), today)?
			};
			Ok(Command::Reasons { span })
		}
		"undo" => {
			if args.len() != 1 {
				return Err("undo takes no arguments".into());
			}
			Ok(Command::Undo)
		}
		"path" => Ok(Command::Path),
		"doctor" => Ok(Command::Doctor),
		"at" => parse_log(args, now),
		other => {
			if parse_score(other).is_ok() {
				parse_log(args, now)
			} else {
				Err(format!("unknown command '{other}'\n\n{}", help()))
			}
		}
	}
}

pub fn help() -> &'static str {
	"mood — log a mood and look at it later

score is 0 (worst) to 10 (best). times are local and 24-hour.
reasons are split on commas: mood 3 work, bad sleep

  mood                         today's average
  mood <score> [reason, reason...]
  mood <score> at <time> [reason, ...]
  mood at <time> <score> [reason, ...]
                               backdate: at yesterday 21:00, at friday 9, at last friday,
                               at 19.10 20:00, at 19.10.2026, at 2026-10-19 18:30
  mood reasons [when]          how each reason sits against your average (last 30 days)
  mood stats [when] [--bucket timeline] [--kind line]
                               mood s works too
  mood graph [when] [directory-or.png] [--bucket hour] [--kind line]
                               on its own it asks when, grouping, chart, and where to save
  mood undo
  mood path
  mood doctor
  mood version

when
  today, yesterday, tomorrow
  week, last week, month, last month, year, last year
  monday … sunday, last friday
  7d, 2w, 3m, 1y, past week
  2026-10-20, 2026-10, 2026
  2026-10-01..2026-10-20

monday is the latest Monday on or before today. last monday is the one before that.
week, month, and year run from the start of that period through today.

buckets (--bucket, or a key inside mood stats)
  hour       one point per hour of the day, one line per day (graph default)
  exact      every log at its exact minute, one line per day
  timeline   one line through time, to the minute when it fits (5 min for a week),
             each day marked (stats default)
  day, week, month, year, weekday

charts (--kind, or 1-7 inside mood stats)
  line (default), step, scatter, bar, area, heatmap, histogram

stats keys
  ↑ ↓        longer or shorter span
             1, 2, 3, 5, 7, 14, 21, 30, 60, 90, 180, 365 days
  ← →        move the window. shift moves one day
  1..7       line, step, scatter, bar, area, heatmap, histogram
  h e t d w m y u
             hour, exact, timeline, day, week, month, year, weekday
  tab        log, averages, or reasons
  o          sort the log by time, then highest, then lowest
  j k        scroll
  home       put today at the end of the window
  r          reload
  q          quit

logs are JSON lines, one <computer>.jsonl per machine, in $MOOD_DIR, else
~/syncthing/mood when that folder exists, else $XDG_DATA_HOME/mood (~/.local/share/mood).
several logs in one hour are kept. hour charts use the average, exact shows each.
empty hours are left out, not drawn as zero. lines connect the logs you have."
}

fn has_help(args: &[String]) -> bool {
	args.iter().any(|arg| arg == "--help" || arg == "-h")
}

struct Flags {
	kind: ChartKind,
	bucket: Bucket,
	positionals: Vec<String>,
}

fn split_flags(args: &[String], mut bucket: Bucket) -> Result<Flags, String> {
	let mut kind = ChartKind::Line;
	let mut positionals = Vec::new();
	let mut index = 0;
	while index < args.len() {
		let arg = &args[index];
		if let Some(value) = arg.strip_prefix("--kind=") {
			kind = ChartKind::parse(value)?;
			index += 1;
			continue;
		}
		if let Some(value) = arg.strip_prefix("--bucket=") {
			bucket = Bucket::parse(value)?;
			index += 1;
			continue;
		}
		if arg == "--kind" || arg == "-k" {
			let value = args.get(index + 1).ok_or("missing value after --kind")?;
			kind = ChartKind::parse(value)?;
			index += 2;
			continue;
		}
		if arg == "--bucket" || arg == "-b" {
			let value = args.get(index + 1).ok_or("missing value after --bucket")?;
			bucket = Bucket::parse(value)?;
			index += 2;
			continue;
		}
		if arg.starts_with('-') && arg != "-" {
			return Err(format!("unknown option '{arg}'"));
		}
		positionals.push(arg.clone());
		index += 1;
	}
	Ok(Flags {
		kind,
		bucket,
		positionals,
	})
}

fn parse_stats(args: &[String], today: NaiveDate) -> Result<Command, String> {
	let flags = split_flags(args, Bucket::Timeline)?;
	let when = if flags.positionals.is_empty() {
		"today".into()
	} else {
		flags.positionals.join(" ")
	};
	Ok(Command::Stats {
		span: range::parse_when(&when, today)?,
		bucket: flags.bucket,
		kind: flags.kind,
	})
}

fn parse_graph(args: &[String], today: NaiveDate) -> Result<Command, String> {
	let flags = split_flags(args, Bucket::HourOfDay)?;
	let (span, out) = split_graph(&flags.positionals, today)?;
	Ok(Command::Graph {
		span,
		bucket: flags.bucket,
		kind: flags.kind,
		out,
	})
}

/// Enter keeps the bracketed answer.
pub fn ask_graph(
	input: &mut impl BufRead,
	output: &mut impl Write,
	today: NaiveDate,
	bucket: Bucket,
	kind: ChartKind,
) -> Result<(Span, Bucket, ChartKind, std::path::PathBuf), String> {
	let span = ask(
		input,
		output,
		"when? today, yesterday, week, last week, month, 7d, 2026-10 …",
		"today",
		|answer| range::parse_when(answer, today),
	)?;
	let bucket = ask(
		input,
		output,
		"group by? hour, exact, timeline, day, week, month, year, weekday",
		bucket.slug(),
		Bucket::parse,
	)?;
	let kind = ask(
		input,
		output,
		"chart? line, step, scatter, bar, area, heatmap, histogram",
		kind.slug(),
		ChartKind::parse,
	)?;
	let default_out = plot::output_path(None, span, bucket, kind);
	let out = ask(
		input,
		output,
		"save to? folder or .png",
		&default_out.to_string_lossy(),
		|answer| Ok::<_, String>(plot::output_path(Some(expand_path(answer)), span, bucket, kind)),
	)?;
	Ok((span, bucket, kind, out))
}

fn ask<T>(
	input: &mut impl BufRead,
	output: &mut impl Write,
	question: &str,
	default: &str,
	parse: impl Fn(&str) -> Result<T, String>,
) -> Result<T, String> {
	loop {
		write!(output, "{question}\n  [{default}] ")
			.and_then(|()| output.flush())
			.map_err(|err| err.to_string())?;
		let mut line = String::new();
		if input.read_line(&mut line).map_err(|err| err.to_string())? == 0 {
			return Err("cancelled".into());
		}
		let answer = line.trim();
		match parse(if answer.is_empty() { default } else { answer }) {
			Ok(value) => return Ok(value),
			Err(err) => writeln!(output, "  {err}").map_err(|err| err.to_string())?,
		}
	}
}

fn split_graph(positionals: &[String], today: NaiveDate) -> Result<(Span, Option<std::path::PathBuf>), String> {
	if positionals.is_empty() {
		return Ok((Span::day(today), None));
	}
	let joined = positionals.join(" ");
	if range::parse_when(&joined, today).is_ok()
		&& !(positionals.len() >= 2 && is_strong_output(positionals.last().unwrap()))
	{
		return Ok((range::parse_when(&joined, today)?, None));
	}
	if positionals.len() >= 2 {
		let when = positionals[..positionals.len() - 1].join(" ");
		if let Ok(span) = range::parse_when(&when, today) {
			return Ok((span, Some(expand_path(positionals.last().unwrap()))));
		}
	}
	if positionals.len() == 1
		&& (is_strong_output(&positionals[0]) || std::path::Path::new(&expand_path(&positionals[0])).is_dir())
	{
		return Ok((Span::day(today), Some(expand_path(&positionals[0]))));
	}
	Ok((range::parse_when(&joined, today)?, None))
}

fn is_strong_output(value: &str) -> bool {
	let lower = value.to_ascii_lowercase();
	value.starts_with('/')
		|| value.starts_with("~/")
		|| value == "~"
		|| value.starts_with("./")
		|| value.starts_with("../")
		|| value == "."
		|| value == ".."
		|| value.contains('/')
		|| lower.ends_with(".png")
		|| lower.ends_with(".svg")
		|| lower.ends_with(".pdf")
		|| lower.ends_with(".jpg")
		|| lower.ends_with(".jpeg")
		|| lower.ends_with(".webp")
}

pub fn expand_path(value: &str) -> std::path::PathBuf {
	if value == "~" {
		if let Some(home) = std::env::var_os("HOME") {
			return std::path::PathBuf::from(home);
		}
	}
	if let Some(rest) = value.strip_prefix("~/") {
		if let Some(home) = std::env::var_os("HOME") {
			return std::path::PathBuf::from(home).join(rest);
		}
	}
	std::path::PathBuf::from(value)
}

fn parse_log(args: &[String], now: DateTime<Local>) -> Result<Command, String> {
	if args.first().map(String::as_str) == Some("at") {
		let (at, used) = take_when(&args[1..], now)?;
		let rest = &args[1 + used..];
		let score = rest.first().ok_or("missing score after the time")?;
		let mood = parse_score(score)?;
		let reasons = split_reasons(&rest[1..].join(" "));
		return Ok(Command::Log { mood, reasons, at });
	}
	let mood = parse_score(&args[0])?;
	if args.get(1).map(String::as_str) == Some("at") && looks_like_when(&args[2..], now) {
		let (at, used) = take_when(&args[2..], now)?;
		let reasons = split_reasons(&args[2 + used..].join(" "));
		return Ok(Command::Log { mood, reasons, at });
	}
	Ok(Command::Log {
		mood,
		reasons: split_reasons(&args[1..].join(" ")),
		at: now,
	})
}

pub fn parse_score(raw: &str) -> Result<f64, String> {
	let text = raw.trim().replace(',', ".");
	let value: f64 = text
		.parse()
		.map_err(|_| format!("'{raw}' is not a score from 0 to 10"))?;
	if !value.is_finite() || !(0.0..=10.0).contains(&value) {
		return Err(format!("score must be from 0 to 10, got {raw}"));
	}
	Ok(value)
}

fn looks_like_when(tokens: &[String], now: DateTime<Local>) -> bool {
	let Some(token) = tokens.first() else {
		return false;
	};
	parse_stamp(token, now).is_some() || parse_clock(token).is_some() || day_of(tokens, now.date_naive()).is_some()
}

fn take_when(tokens: &[String], now: DateTime<Local>) -> Result<(DateTime<Local>, usize), String> {
	if tokens.is_empty() {
		return Err("missing time after 'at'. example: mood 7 at 18:30".into());
	}
	if let Some(parsed) = parse_stamp(&tokens[0], now) {
		return Ok((parsed?, 1));
	}
	if let Some(time) = parse_clock(&tokens[0]) {
		return Ok((combine(now.date_naive(), time, now)?, 1));
	}
	if let Some((date, used)) = day_of(tokens, now.date_naive()) {
		if let Some(time) = tokens.get(used).and_then(|token| parse_clock(token)) {
			return Ok((combine(date, time, now)?, used + 1));
		}
		return Ok((combine(date, now.time(), now)?, used));
	}
	Err(format!(
		"could not read '{}' as a time. examples: 18:30, yesterday 21:00, friday 9, 19.10 20:00, 2026-10-19 18:30",
		tokens[0]
	))
}

fn parse_stamp(value: &str, now: DateTime<Local>) -> Option<Result<DateTime<Local>, String>> {
	if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
		return Some(finish(dt.with_timezone(&Local), now));
	}
	for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
		if let Ok(naive) = NaiveDateTime::parse_from_str(value, fmt) {
			return Some(combine_naive(naive, now));
		}
	}
	None
}

/// Also returns the tokens used: "last friday" is two.
fn day_of(tokens: &[String], today: NaiveDate) -> Option<(NaiveDate, usize)> {
	if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("last") {
		let span = range::parse_when(&tokens[..2].join(" "), today).ok()?;
		return (span.start == span.end).then_some((span.start, 2));
	}
	let token = tokens.first()?;
	if let Some(date) = dotted_date(token, today) {
		return Some((date, 1));
	}
	// Anything `mood stats` reads as one day.
	let span = range::parse_when(token, today).ok()?;
	(span.start == span.end).then_some((span.start, 1))
}

/// Without a year, the latest 19.10 up to today.
fn dotted_date(value: &str, today: NaiveDate) -> Option<NaiveDate> {
	let parts: Vec<u32> = value
		.trim_end_matches('.')
		.split('.')
		.map(|part| part.parse().ok())
		.collect::<Option<_>>()?;
	match parts.as_slice() {
		[day, month, year] => NaiveDate::from_ymd_opt(*year as i32, *month, *day),
		[day, month] => {
			let date = NaiveDate::from_ymd_opt(today.year(), *month, *day)?;
			if date > today {
				NaiveDate::from_ymd_opt(today.year() - 1, *month, *day)
			} else {
				Some(date)
			}
		}
		_ => None,
	}
}

fn parse_clock(value: &str) -> Option<NaiveTime> {
	let parts: Vec<&str> = value.split(':').collect();
	let numbers = parts
		.iter()
		.map(|part| part.parse::<u32>().ok())
		.collect::<Option<Vec<_>>>()?;
	if parts
		.iter()
		.any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
	{
		return None;
	}
	match numbers.as_slice() {
		[hour] if *hour <= 23 && parts[0].len() <= 2 => NaiveTime::from_hms_opt(*hour, 0, 0),
		[hour, minute] if *hour <= 23 && *minute <= 59 => NaiveTime::from_hms_opt(*hour, *minute, 0),
		[hour, minute, second] if *hour <= 23 && *minute <= 59 && *second <= 59 => {
			NaiveTime::from_hms_opt(*hour, *minute, *second)
		}
		_ => None,
	}
}

fn combine(date: NaiveDate, time: NaiveTime, now: DateTime<Local>) -> Result<DateTime<Local>, String> {
	combine_naive(NaiveDateTime::new(date, time), now)
}

fn combine_naive(naive: NaiveDateTime, now: DateTime<Local>) -> Result<DateTime<Local>, String> {
	let dt = match Local.from_local_datetime(&naive) {
		LocalResult::Single(dt) => dt,
		LocalResult::Ambiguous(dt, _) => dt,
		LocalResult::None => {
			return Err(format!(
				"{} does not exist in local time",
				naive.format("%Y-%m-%d %H:%M")
			));
		}
	};
	finish(dt, now)
}

fn finish(dt: DateTime<Local>, now: DateTime<Local>) -> Result<DateTime<Local>, String> {
	if dt > now + Duration::seconds(90) {
		return Err(format!("that time is in the future ({})", dt.format("%Y-%m-%d %H:%M")));
	}
	Ok(dt)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn now() -> DateTime<Local> {
		Local.with_ymd_and_hms(2026, 10, 20, 15, 0, 0).single().expect("time")
	}

	fn args(parts: &[&str]) -> Vec<String> {
		parts.iter().map(|part| (*part).to_string()).collect()
	}

	#[test]
	fn stats_yesterday_and_graph_directory() {
		let stats = parse(&args(&["stats", "yesterday"]), now()).unwrap();
		match stats {
			Command::Stats { span, bucket, kind } => {
				assert_eq!(span.end, NaiveDate::from_ymd_opt(2026, 10, 19).unwrap());
				assert_eq!(bucket, Bucket::Timeline);
				assert_eq!(kind, ChartKind::Line);
			}
			_ => panic!("stats"),
		}
		assert!(matches!(parse(&args(&["s"]), now()).unwrap(), Command::Stats { .. }));
		let graph = parse(&args(&["graph", "last", "week", "/tmp/mood-out"]), now()).unwrap();
		match graph {
			Command::Graph { span, bucket, out, .. } => {
				assert_eq!(bucket, Bucket::HourOfDay);
				assert_eq!(span.start, NaiveDate::from_ymd_opt(2026, 10, 12).unwrap());
				assert_eq!(out.unwrap(), std::path::PathBuf::from("/tmp/mood-out"));
			}
			_ => panic!("graph"),
		}
	}

	#[test]
	fn graph_questions_take_defaults_and_retry() {
		let today = now().date_naive();
		let mut answers = std::io::Cursor::new("last week\nnope\n\nbar\n/tmp/mood-out\n");
		let mut shown = Vec::new();
		let (span, bucket, kind, out) =
			ask_graph(&mut answers, &mut shown, today, Bucket::HourOfDay, ChartKind::Line).unwrap();
		assert_eq!(span.start, NaiveDate::from_ymd_opt(2026, 10, 12).unwrap());
		assert_eq!(bucket, Bucket::HourOfDay);
		assert_eq!(kind, ChartKind::Bar);
		assert_eq!(
			out,
			std::path::PathBuf::from("/tmp/mood-out/mood-2026-10-12--2026-10-18-hour-bar.png")
		);
		assert!(String::from_utf8(shown).unwrap().contains("unknown bucket 'nope'"));
		let mut cut_short = std::io::Cursor::new("today\n");
		assert!(
			ask_graph(
				&mut cut_short,
				&mut Vec::new(),
				today,
				Bucket::HourOfDay,
				ChartKind::Line
			)
			.is_err()
		);
	}

	#[test]
	fn log_with_clock_and_comma() {
		let command = parse(&args(&["6,5", "at", "08:15", "morning"]), now()).unwrap();
		match command {
			Command::Log { mood, reasons, at } => {
				assert!((mood - 6.5).abs() < 1e-9);
				assert_eq!(reasons, ["morning"]);
				assert_eq!(
					at.naive_local().format("%Y-%m-%d %H:%M").to_string(),
					"2026-10-20 08:15"
				);
			}
			_ => panic!("log"),
		}
	}

	#[test]
	fn at_inside_a_note_is_not_a_time() {
		let command = parse(&args(&["7", "at", "the", "store"]), now()).unwrap();
		match command {
			Command::Log { mood, reasons, .. } => {
				assert!((mood - 7.0).abs() < 1e-9);
				assert_eq!(reasons, ["at the store"]);
			}
			_ => panic!("log"),
		}
	}

	#[test]
	fn reasons_split_on_commas() {
		match parse(&args(&["3", "Work,", "bad", "sleep"]), now()).unwrap() {
			Command::Log { reasons, .. } => assert_eq!(reasons, ["work", "bad sleep"]),
			_ => panic!("log"),
		}
	}

	#[test]
	fn backdates_to_weekdays_and_dotted_dates() {
		let at = |parts: &[&str]| match parse(&args(parts), now()).unwrap() {
			Command::Log { at, reasons, .. } => (at.naive_local().format("%Y-%m-%d %H:%M").to_string(), reasons),
			_ => panic!("log"),
		};
		// now() is Tuesday 2026-10-20 15:00
		assert_eq!(at(&["6", "at", "friday", "21:00", "tired"]).0, "2026-10-16 21:00");
		assert_eq!(at(&["6", "at", "last", "monday", "9"]).0, "2026-10-12 09:00");
		assert_eq!(at(&["6", "at", "19.10", "20:30"]).0, "2026-10-19 20:30");
		assert_eq!(at(&["6", "at", "24.12"]).0, "2025-12-24 15:00");
		assert_eq!(
			at(&["at", "1.10.2026", "8", "5", "gym"]),
			("2026-10-01 08:00".into(), vec!["gym".to_string()])
		);
		assert_eq!(at(&["7", "at", "last", "orders"]).1, ["at last orders"]);
		assert!(
			parse(&args(&["6", "at", "week"]), now())
				.is_ok_and(|c| matches!(c, Command::Log { ref reasons, .. } if reasons == &["at week"]))
		);
	}

	#[test]
	fn rejects_a_future_time() {
		let err = parse(&args(&["4", "at", "18:00"]), now()).unwrap_err();
		assert!(err.contains("future"), "{err}");
	}
}
