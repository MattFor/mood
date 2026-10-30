use chrono::{DateTime, Local, LocalResult, NaiveDateTime, SecondsFormat, TimeZone};
use serde::{Deserialize, Serialize};

/// `at` is local wall time, `t` the stored timestamp.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
	pub at: NaiveDateTime,
	pub t: String,
	pub mood: f64,
	pub reasons: Vec<String>,
	pub host: String,
}

#[derive(Serialize, Deserialize)]
struct Raw {
	t: String,
	mood: f64,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	reasons: Vec<String>,
	// Pre-reasons logs carry a free-text note, read as reasons.
	#[serde(default, skip_serializing)]
	note: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	host: String,
}

impl Entry {
	pub fn new(at: DateTime<Local>, mood: f64, reasons: Vec<String>, host: String) -> Self {
		Self {
			at: at.naive_local(),
			t: at.to_rfc3339_opts(SecondsFormat::Secs, false),
			mood,
			reasons,
			host,
		}
	}

	pub fn to_line(&self) -> Result<String, String> {
		let local = match Local.from_local_datetime(&self.at) {
			LocalResult::Single(dt) => dt,
			LocalResult::Ambiguous(dt, _) => dt,
			LocalResult::None => {
				return Err(format!(
					"{} does not exist in local time",
					self.at.format("%Y-%m-%d %H:%M")
				));
			}
		};
		let raw = Raw {
			t: local.to_rfc3339_opts(SecondsFormat::Secs, false),
			mood: self.mood,
			reasons: self.reasons.clone(),
			note: String::new(),
			host: self.host.clone(),
		};
		serde_json::to_string(&raw).map_err(|err| err.to_string())
	}
}

pub fn parse_line(line: &str) -> Result<Option<Entry>, String> {
	let line = line.trim();
	if line.is_empty() {
		return Ok(None);
	}
	let raw: Raw = serde_json::from_str(line).map_err(|err| format!("bad log line: {err}"))?;
	if !raw.mood.is_finite() {
		return Err("mood is not a number".into());
	}
	let at = parse_timestamp(&raw.t)?;
	let mut reasons = raw.reasons;
	reasons.push(raw.note);
	Ok(Some(Entry {
		at,
		t: raw.t,
		mood: raw.mood,
		reasons: split_reasons(&reasons.join(",")),
		host: raw.host,
	}))
}

/// Lowercased so the same reason groups together.
pub fn split_reasons(text: &str) -> Vec<String> {
	let mut reasons: Vec<String> = Vec::new();
	for part in text.split(',') {
		let reason: String = part
			.split_whitespace()
			.collect::<Vec<_>>()
			.join(" ")
			.to_lowercase()
			.chars()
			.take(80)
			.collect();
		if !reason.is_empty() && !reasons.contains(&reason) {
			reasons.push(reason);
		}
	}
	reasons
}

pub fn parse_log_text(text: &str) -> (Vec<Entry>, usize) {
	let mut entries = Vec::new();
	let mut skipped = 0usize;
	for line in text.lines() {
		match parse_line(line) {
			Ok(Some(entry)) => entries.push(entry),
			Ok(None) => {}
			Err(_) => skipped += 1,
		}
	}
	(entries, skipped)
}

fn parse_timestamp(raw: &str) -> Result<NaiveDateTime, String> {
	if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
		return Ok(dt.with_timezone(&Local).naive_local());
	}
	for fmt in [
		"%Y-%m-%dT%H:%M:%S",
		"%Y-%m-%dT%H:%M",
		"%Y-%m-%d %H:%M:%S",
		"%Y-%m-%d %H:%M",
	] {
		if let Ok(naive) = NaiveDateTime::parse_from_str(raw, fmt) {
			return Ok(naive);
		}
	}
	Err(format!("could not read timestamp '{raw}'"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roundtrip_keeps_local_time() {
		let at = Local.with_ymd_and_hms(2026, 10, 20, 14, 5, 0).single().expect("time");
		let entry = Entry::new(at, 6.5, vec!["work".into(), "bad sleep".into()], "desktop".into());
		let line = entry.to_line().unwrap();
		assert!(!line.contains("note"), "{line}");
		let back = parse_line(&line).unwrap().unwrap();
		assert_eq!(back.at, entry.at);
		assert_eq!(back.mood, 6.5);
		assert_eq!(back.reasons, ["work", "bad sleep"]);
		assert_eq!(back.host, "desktop");
	}

	#[test]
	fn old_note_reads_as_reasons() {
		let back = parse_line("{\"t\":\"2026-10-20T09:00:00+02:00\",\"mood\":4,\"note\":\"Work,  bad sleep\"}")
			.unwrap()
			.unwrap();
		assert_eq!(back.reasons, ["work", "bad sleep"]);
		assert_eq!(split_reasons(" Gym, gym ,, "), ["gym"]);
	}

	#[test]
	fn skips_broken_lines() {
		let text = "{\"t\":\"2026-10-20T09:00:00+02:00\",\"mood\":4,\"host\":\"z\"}\nnot json\n\n";
		let (entries, skipped) = parse_log_text(text);
		assert_eq!(entries.len(), 1);
		assert_eq!(skipped, 1);
		assert_eq!(entries[0].mood, 4.0);
	}
}
