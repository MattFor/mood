use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate, Timelike};

use crate::agg::{self, fmt_num};
use crate::entry::{self, Entry};
use crate::kind::Bucket;
use crate::range::Span;

const FORMAT: &str = "\
Mood logs shared between computers.

Scale: 0 is the worst, 10 is the best.
Clock: local time, stored as RFC3339, always shown in 24-hour form.

Each computer appends to its own file so a sync client does not have to merge one log:
  desktop.jsonl
  laptop.jsonl

One JSON object per line:
  {\"t\":\"2026-10-20T14:05:00+02:00\",\"mood\":6.5,\"reasons\":[\"work\",\"bad sleep\"],\"host\":\"desktop\"}

reasons is optional, lowercase, and never holds commas.
Older lines may have a free-text \"note\" instead. It is read as reasons.

summary.json is rebuilt from those files.
graphs/ holds PNG charts from `mood graph`.

Several logs in one hour are kept. Charts use their average.
A missing hour is a gap, not a zero.
";

pub fn data_dir() -> PathBuf {
	resolve_data_dir(
		std::env::var_os("MOOD_DIR"),
		std::env::var_os("HOME"),
		std::env::var_os("XDG_DATA_HOME"),
	)
}

fn resolve_data_dir(mood_dir: Option<OsString>, home: Option<OsString>, xdg_data_home: Option<OsString>) -> PathBuf {
	if let Some(dir) = mood_dir.filter(|dir| !dir.is_empty()) {
		return PathBuf::from(dir);
	}
	let home = home.map(PathBuf::from).unwrap_or_default();
	let synced = home.join("syncthing").join("mood");
	if synced.is_dir() {
		return synced;
	}
	match xdg_data_home.map(PathBuf::from).filter(|dir| dir.is_absolute()) {
		Some(dir) => dir.join("mood"),
		None => home.join(".local/share/mood"),
	}
}

pub fn graph_dir() -> PathBuf {
	data_dir().join("graphs")
}

pub fn machine_name() -> String {
	// Android has no /etc/hostname, so phones set MOOD_HOST.
	let raw = std::env::var("MOOD_HOST").ok().filter(|name| !name.is_empty());
	let raw = raw.unwrap_or_else(|| fs::read_to_string("/etc/hostname").unwrap_or_default());
	let raw = raw.trim();
	let raw = if raw.is_empty() {
		std::env::var("HOSTNAME").unwrap_or_else(|_| "host".into())
	} else {
		raw.to_string()
	};
	let name: String = raw
		.chars()
		.flat_map(|c| c.to_lowercase())
		.filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
		.collect();
	if name.is_empty() { "host".into() } else { name }
}

pub fn host_log_path() -> PathBuf {
	data_dir().join(format!("{}.jsonl", machine_name()))
}

pub fn ensure() -> Result<(), String> {
	let dir = data_dir();
	fs::create_dir_all(&dir).map_err(|err| format!("could not create {}: {err}", dir.display()))?;
	let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
	let graphs = dir.join("graphs");
	fs::create_dir_all(&graphs).map_err(|err| format!("could not create {}: {err}", graphs.display()))?;
	let format_path = dir.join("FORMAT.txt");
	if fs::read_to_string(&format_path).ok().as_deref() != Some(FORMAT) {
		let _ = fs::write(&format_path, FORMAT);
	}
	Ok(())
}

pub fn load() -> Result<Vec<Entry>, String> {
	let dir = data_dir();
	if !dir.exists() {
		return Ok(Vec::new());
	}
	let mut entries = Vec::new();
	let mut skipped = 0usize;
	for path in log_files(&dir) {
		let text = fs::read_to_string(&path).map_err(|err| format!("could not read {}: {err}", path.display()))?;
		let (mut parsed, bad) = entry::parse_log_text(&text);
		skipped += bad;
		entries.append(&mut parsed);
	}
	if entries.is_empty() && skipped > 0 {
		return Err(format!("{skipped} log lines could not be read"));
	}
	entries.sort_by(|a, b| {
		a.at.cmp(&b.at)
			.then(a.host.cmp(&b.host))
			.then(a.reasons.cmp(&b.reasons))
			.then(a.mood.partial_cmp(&b.mood).unwrap_or(std::cmp::Ordering::Equal))
	});
	entries.dedup_by(|a, b| a.at == b.at && a.host == b.host && a.reasons == b.reasons && a.mood == b.mood);
	Ok(entries)
}

fn log_files(dir: &Path) -> Vec<PathBuf> {
	let mut files = Vec::new();
	let Ok(read) = fs::read_dir(dir) else {
		return files;
	};
	for item in read.flatten() {
		let name = item.file_name().to_string_lossy().to_string();
		if name.starts_with('.') || name.contains("sync-conflict") || !name.ends_with(".jsonl") {
			continue;
		}
		files.push(item.path());
	}
	files.sort();
	files
}

pub fn log_mood(mood: f64, reasons: Vec<String>, at: chrono::DateTime<Local>) -> Result<String, String> {
	ensure()?;
	let entry = Entry::new(at, mood, reasons, machine_name());
	append(&entry)?;
	let entries = load()?;
	let when = if entry.at.date() == Local::now().date_naive() {
		"%H:%M"
	} else {
		"%a %Y-%m-%d %H:%M"
	};
	let mut message = format!(
		"logged {} at {}{}",
		fmt_num(entry.mood),
		entry.at.format(when),
		because(&entry)
	);
	let same: Vec<f64> = entries
		.iter()
		.filter(|item| item.at.date() == entry.at.date() && item.at.hour() == entry.at.hour())
		.map(|item| item.mood)
		.collect();
	if same.len() > 1 {
		let average = same.iter().sum::<f64>() / same.len() as f64;
		message.push_str(&format!(
			" ({} logs this hour, average {})",
			same.len(),
			fmt_num(average)
		));
	}
	if let Err(err) = write_summary(&entries) {
		message.push_str(&format!(" (summary not updated: {err})"));
	}
	Ok(message)
}

pub fn undo() -> Result<String, String> {
	ensure()?;
	let path = host_log_path();
	let text = fs::read_to_string(&path).map_err(|_| "nothing to undo".to_string())?;
	let mut lines: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
	let last = lines.pop().ok_or_else(|| "nothing to undo".to_string())?;
	let removed = entry::parse_line(last).ok().flatten();
	let mut body = lines.join("\n");
	if !body.is_empty() {
		body.push('\n');
	}
	atomic_write(&path, body.as_bytes())?;
	let entries = load()?;
	let _ = write_summary(&entries);
	match removed {
		Some(entry) => Ok(format!(
			"removed {} at {}{}",
			fmt_num(entry.mood),
			entry.at.format("%Y-%m-%d %H:%M"),
			because(&entry)
		)),
		None => Ok("removed a broken last line".into()),
	}
}

fn because(entry: &Entry) -> String {
	if entry.reasons.is_empty() {
		String::new()
	} else {
		format!(" — {}", entry.reasons.join(", "))
	}
}

fn append(entry: &Entry) -> Result<(), String> {
	let path = host_log_path();
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).map_err(|err| err.to_string())?;
	}
	let mut file = OpenOptions::new()
		.create(true)
		.append(true)
		.mode(0o600)
		.open(&path)
		.map_err(|err| format!("could not open {}: {err}", path.display()))?;
	writeln!(file, "{}", entry.to_line()?).map_err(|err| err.to_string())?;
	file.sync_all().ok();
	Ok(())
}

pub fn write_summary(entries: &[Entry]) -> Result<(), String> {
	ensure()?;
	let today = Local::now().date_naive();
	let value = summary_document(entries, today);
	let bytes = serde_json::to_vec_pretty(&value).map_err(|err| err.to_string())?;
	atomic_write(&data_dir().join("summary.json"), &bytes)
}

fn portable_data_dir() -> String {
	let directory = data_dir();
	let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
		return directory.display().to_string();
	};

	directory
		.strip_prefix(&home)
		.map(|relative| {
			if relative.as_os_str().is_empty() {
				"~".to_string()
			} else {
				format!("~/{}", relative.display())
			}
		})
		.unwrap_or_else(|_| directory.display().to_string())
}

fn summary_document(entries: &[Entry], today: NaiveDate) -> serde_json::Value {
	let yesterday = today.pred_opt().unwrap_or(today);
	let week = Span::ending(today, 7);
	let month = Span::ending(today, 30);
	// Same content on every device, so the synced cache never conflicts.
	let updated = entries
		.iter()
		.max_by(|a, b| a.at.cmp(&b.at).then_with(|| a.t.cmp(&b.t)))
		.map(|entry| entry.t.as_str())
		.unwrap_or("1970-01-01T00:00:00+00:00");
	let recent: Vec<&Entry> = {
		let mut items: Vec<&Entry> = entries.iter().collect();
		items.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.t.cmp(&b.t)));
		items.truncate(40);
		items
	};
	let today_view = agg::build_view(entries, Span::day(today), Bucket::HourOfDay);
	let days = agg::build_view(entries, month, Bucket::Day);
	let weekdays = agg::build_view(entries, month, Bucket::Weekday);
	serde_json::json!({
		"updated": updated,
		"scale": {"min": 0, "max": 10, "meaning": "0 worst, 10 best"},
		"directory": portable_data_dir(),
		"count": entries.len(),
		"latest": recent.first().map(|entry| entry_json(entry)),
		"today": stat_json(&agg::summary_between(entries, Span::day(today))),
		"yesterday": stat_json(&agg::summary_between(entries, Span::day(yesterday))),
		"last_7_days": stat_json(&agg::summary_between(entries, week)),
		"last_30_days": stat_json(&agg::summary_between(entries, month)),
		"today_by_hour": hour_json(&today_view),
		"last_30_days_by_day": series_json(&days.categories, days.series.first()),
		"last_30_days_by_weekday": series_json(&weekdays.categories, weekdays.series.first()),
		"last_30_days_by_reason": days.reasons.iter().map(|row| serde_json::json!({
			"reason": row.reason,
			"n": row.n,
			"mean": round2(row.mean),
		})).collect::<Vec<_>>(),
		"recent": recent.iter().map(|entry| entry_json(entry)).collect::<Vec<_>>(),
	})
}

fn entry_json(entry: &Entry) -> serde_json::Value {
	serde_json::json!({
		"t": entry.t,
		"mood": round2(entry.mood),
		"reasons": entry.reasons,
		"host": entry.host,
	})
}

fn stat_json(summary: &agg::Summary) -> serde_json::Value {
	if summary.n == 0 {
		return serde_json::json!({"n": 0, "mean": null, "median": null, "min": null, "max": null, "sd": null});
	}
	serde_json::json!({
		"n": summary.n,
		"mean": round2(summary.mean),
		"median": round2(summary.median),
		"min": round2(summary.min),
		"max": round2(summary.max),
		"sd": round2(summary.sd),
	})
}

fn hour_json(view: &agg::View) -> Vec<serde_json::Value> {
	let mut means = [None; 24];
	let mut counts = [0usize; 24];
	for row in &view.rows {
		let Some(hour) = row.label.get(..2).and_then(|text| text.parse::<usize>().ok()) else {
			continue;
		};
		if hour < 24 {
			means[hour] = Some(row.mean);
			counts[hour] = row.n;
		}
	}
	(0..24)
		.map(|hour| {
			serde_json::json!({
				"hour": format!("{hour:02}"),
				"mean": means[hour].map(round2),
				"n": counts[hour],
			})
		})
		.collect()
}

fn series_json(categories: &[String], series: Option<&agg::Series>) -> Vec<serde_json::Value> {
	let empty = Vec::new();
	let values = series.map(|series| series.y.as_slice()).unwrap_or(&empty);
	categories
		.iter()
		.enumerate()
		.map(|(index, label)| {
			let mean = values.get(index).copied().flatten();
			serde_json::json!({
				"label": label,
				"mean": mean.map(round2),
			})
		})
		.collect()
}

fn round2(value: f64) -> f64 {
	(value * 100.0).round() / 100.0
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent).map_err(|err| err.to_string())?;
	}
	let tmp = path.with_extension("tmp");
	{
		let mut file = OpenOptions::new()
			.create(true)
			.write(true)
			.truncate(true)
			.mode(0o600)
			.open(&tmp)
			.map_err(|err| err.to_string())?;
		file.write_all(bytes).map_err(|err| err.to_string())?;
		file.sync_all().ok();
	}
	fs::rename(&tmp, path).map_err(|err| err.to_string())
}

pub fn status_line(entries: &[Entry]) -> String {
	let today = Local::now().date_naive();
	let today_summary = agg::summary_between(entries, Span::day(today));
	let week = agg::summary_between(entries, Span::ending(today, 7));
	let latest = entries.iter().max_by_key(|entry| entry.at);
	let mut lines = Vec::new();
	lines.push("scale  0 worst … 10 best".into());
	match latest {
		Some(entry) => {
			lines.push(format!(
				"latest {} at {}{}",
				fmt_num(entry.mood),
				entry.at.format("%Y-%m-%d %H:%M"),
				because(entry)
			));
		}
		None => lines.push("latest none yet".into()),
	}
	lines.push(format!("today  {}", brief(&today_summary)));
	lines.push(format!("7 days {}", brief(&week)));
	lines.push(format!("data   {}", data_dir().display()));
	lines.push("try    mood 7 work, sleep   ·   mood reasons   ·   mood stats yesterday   ·   mood graph week".into());
	lines.join("\n")
}

pub fn reasons_report(entries: &[Entry], span: Span) -> String {
	let view = agg::build_view(entries, span, Bucket::Day);
	if view.summary.n == 0 {
		return format!("{}    no logs", span.label());
	}
	let mut lines = vec![format!(
		"{}    mean {} over {} logs",
		span.label(),
		fmt_num(view.summary.mean),
		view.summary.n
	)];
	let untagged: Vec<f64> = view
		.logs
		.iter()
		.filter(|entry| entry.reasons.is_empty())
		.map(|entry| entry.mood)
		.collect();
	let width = view
		.reasons
		.iter()
		.map(|row| row.reason.chars().count())
		.max()
		.unwrap_or(0)
		.max(11);
	for row in &view.reasons {
		let delta = row.mean - view.summary.mean;
		let sign = if delta < -0.05 { "-" } else { "+" };
		lines.push(format!(
			"  {:<width$}  n {:>3}  mean {:>4}  {sign}{}",
			row.reason,
			row.n,
			fmt_num(row.mean),
			fmt_num(delta.abs())
		));
	}
	if !untagged.is_empty() {
		let mean = untagged.iter().sum::<f64>() / untagged.len() as f64;
		lines.push(format!(
			"  {:<width$}  n {:>3}  mean {:>4}",
			"(no reason)",
			untagged.len(),
			fmt_num(mean)
		));
	}
	if view.reasons.is_empty() {
		lines.push("no reasons yet. log one with: mood 4 work, bad sleep".into());
	}
	lines.join("\n")
}

fn brief(summary: &agg::Summary) -> String {
	if summary.n == 0 {
		"no logs".into()
	} else {
		format!(
			"n {}  mean {}  median {}  min {}  max {}  sd {}",
			summary.n,
			fmt_num(summary.mean),
			fmt_num(summary.median),
			fmt_num(summary.min),
			fmt_num(summary.max),
			fmt_num(summary.sd)
		)
	}
}

#[cfg(test)]
mod summary_tests {
	use super::*;

	#[test]
	fn shared_summary_metadata_is_deterministic() {
		let entry = Entry {
			at: NaiveDate::from_ymd_opt(2026, 10, 27)
				.unwrap()
				.and_hms_opt(1, 2, 3)
				.unwrap(),
			t: "2026-10-27T01:02:03+01:00".into(),
			mood: 5.0,
			reasons: vec!["test".into()],
			host: "test-host".into(),
		};
		let entries = [entry];
		let today = NaiveDate::from_ymd_opt(2026, 10, 27).unwrap();

		let first = summary_document(&entries, today);
		let second = summary_document(&entries, today);

		assert_eq!(first, second);
		assert_eq!(first["updated"], entries[0].t);
		assert_eq!(first["directory"], portable_data_dir());
	}

	#[test]
	fn data_dir_order() {
		let home = std::env::temp_dir().join(format!("mood-test-home-{}", std::process::id()));
		let os = |path: &std::path::Path| Some(path.as_os_str().to_owned());
		assert_eq!(
			resolve_data_dir(Some("/x".into()), os(&home), None),
			PathBuf::from("/x")
		);
		assert_eq!(
			resolve_data_dir(Some("".into()), os(&home), None),
			home.join(".local/share/mood")
		);
		assert_eq!(
			resolve_data_dir(None, os(&home), Some("/xdg".into())),
			PathBuf::from("/xdg/mood")
		);
		assert_eq!(
			resolve_data_dir(None, os(&home), Some("relative".into())),
			home.join(".local/share/mood")
		);
		fs::create_dir_all(home.join("syncthing")).unwrap();
		assert_eq!(resolve_data_dir(None, os(&home), None), home.join(".local/share/mood"));
		fs::create_dir_all(home.join("syncthing/mood")).unwrap();
		assert_eq!(
			resolve_data_dir(None, os(&home), Some("/xdg".into())),
			home.join("syncthing/mood")
		);
		fs::remove_dir_all(&home).unwrap();
	}

	#[test]
	fn empty_summary_has_stable_update_value() {
		let summary = summary_document(&[], NaiveDate::from_ymd_opt(2026, 10, 27).unwrap());
		assert_eq!(summary["updated"], "1970-01-01T00:00:00+00:00");
	}
}
