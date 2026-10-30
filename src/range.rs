use chrono::{Datelike, Duration, Months, NaiveDate, Weekday};

/// Inclusive local-date window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
	pub start: NaiveDate,
	pub end: NaiveDate,
}

const LADDER: &[i64] = &[1, 2, 3, 5, 7, 14, 21, 30, 60, 90, 180, 365, 730, 1825];

impl Span {
	pub fn new(start: NaiveDate, end: NaiveDate) -> Self {
		if start <= end {
			Self { start, end }
		} else {
			Self { start: end, end: start }
		}
	}

	pub fn day(date: NaiveDate) -> Self {
		Self { start: date, end: date }
	}

	pub fn len_days(self) -> i64 {
		(self.end - self.start).num_days() + 1
	}

	pub fn with_len_ending(end: NaiveDate, len: i64) -> Self {
		let len = len.max(1);
		let start = end.checked_sub_signed(Duration::days(len - 1)).unwrap_or(end);
		Self { start, end }
	}

	pub fn ending(end: NaiveDate, days: i64) -> Self {
		Self::with_len_ending(end, days)
	}

	pub fn label(self) -> String {
		if self.start == self.end {
			self.start.format("%Y-%m-%d").to_string()
		} else {
			format!("{} → {}", self.start.format("%Y-%m-%d"), self.end.format("%Y-%m-%d"))
		}
	}

	pub fn dates(self) -> Vec<NaiveDate> {
		let mut out = Vec::new();
		let mut date = self.start;
		loop {
			out.push(date);
			if date >= self.end {
				break;
			}
			match date.succ_opt() {
				Some(next) => date = next,
				None => break,
			}
		}
		out
	}

	pub fn widen(self) -> Self {
		Self::with_len_ending(self.end, next_len(self.len_days()))
	}

	pub fn shorten(self) -> Self {
		Self::with_len_ending(self.end, prev_len(self.len_days()))
	}

	/// Positive moves toward today; the end never passes `today`.
	pub fn pan(self, direction: i64, fine: bool, today: NaiveDate) -> Self {
		let step = if fine || self.len_days() <= 1 {
			1
		} else {
			(self.len_days() / 2).max(1)
		};
		let delta = direction.signum() * step;
		let end = self.end.checked_add_signed(Duration::days(delta)).unwrap_or(self.end);
		let end = if end > today { today } else { end };
		Self::with_len_ending(end, self.len_days())
	}

	pub fn end_on(self, today: NaiveDate) -> Self {
		Self::with_len_ending(today, self.len_days())
	}
}

fn next_len(len: i64) -> i64 {
	LADDER.iter().copied().find(|days| *days > len).unwrap_or(1825)
}

fn prev_len(len: i64) -> i64 {
	LADDER.iter().copied().rev().find(|days| *days < len).unwrap_or(1)
}

pub fn parse_when(input: &str, today: NaiveDate) -> Result<Span, String> {
	let raw = input.trim();
	if raw.is_empty() || eq(raw, "today") || eq(raw, "now") {
		return Ok(Span::day(today));
	}
	if let Some((start, end)) = split_range(raw) {
		return Ok(Span::new(parse_date(start)?, parse_date(end)?));
	}
	let key = raw
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
		.to_ascii_lowercase();
	if let Some(span) = phrase(&key, today) {
		return Ok(span);
	}
	if let Some(span) = relative(&key, today)? {
		return Ok(span);
	}
	if let Some(span) = calendar_token(raw.trim(), today)? {
		return Ok(span);
	}
	Err(format!(
		"could not read '{raw}' as a time range\nexamples: today, yesterday, week, last week, month, year, monday, 7d, 2w, 3m, 2026-10-20, 2026-10-01..2026-10-20"
	))
}

fn eq(value: &str, expected: &str) -> bool {
	value.eq_ignore_ascii_case(expected)
}

fn split_range(raw: &str) -> Option<(&str, &str)> {
	if let Some((start, end)) = raw.split_once("..") {
		return Some((start.trim(), end.trim()));
	}
	let (start, end) = raw.split_once(" to ")?;
	let start = start.trim();
	let end = end.trim();
	if looks_like_date(start) && looks_like_date(end) {
		Some((start, end))
	} else {
		None
	}
}

fn looks_like_date(value: &str) -> bool {
	let bytes = value.as_bytes();
	bytes.len() >= 10 && bytes[4] == b'-' && bytes[0].is_ascii_digit()
}

fn parse_date(value: &str) -> Result<NaiveDate, String> {
	NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(|_| format!("'{value}' is not a date (YYYY-MM-DD)"))
}

fn phrase(key: &str, today: NaiveDate) -> Option<Span> {
	match key {
		"yesterday" => Some(Span::day(today.pred_opt()?)),
		"tomorrow" => Some(Span::day(today.succ_opt()?)),
		"week" | "this week" => Some(week_to_date(today)),
		"last week" => Some(last_week(today)),
		"month" | "this month" => Some(month_to_date(today)),
		"last month" => Some(last_month(today)),
		"year" | "this year" => Some(year_to_date(today)),
		"last year" => Some(last_year(today)),
		"past week" => Some(Span::ending(today, 7)),
		"past month" => Some(months_back(today, 1)),
		"past year" => Some(months_back(today, 12)),
		_ => weekday_phrase(key, today),
	}
}

fn week_to_date(today: NaiveDate) -> Span {
	let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
	Span::new(monday, today)
}

fn last_week(today: NaiveDate) -> Span {
	let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
	Span::new(monday - Duration::days(7), monday - Duration::days(1))
}

fn month_to_date(today: NaiveDate) -> Span {
	let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
	Span::new(start, today)
}

fn last_month(today: NaiveDate) -> Span {
	let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
	let end = first.pred_opt().unwrap_or(today);
	let start = NaiveDate::from_ymd_opt(end.year(), end.month(), 1).unwrap_or(end);
	Span::new(start, end)
}

fn year_to_date(today: NaiveDate) -> Span {
	let start = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
	Span::new(start, today)
}

fn last_year(today: NaiveDate) -> Span {
	let year = today.year() - 1;
	Span::new(
		NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(today),
		NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(today),
	)
}

fn weekday_phrase(key: &str, today: NaiveDate) -> Option<Span> {
	let (previous, name) = if let Some(rest) = key.strip_prefix("last ") {
		(true, rest)
	} else {
		(false, key)
	};
	let weekday = weekday_name(name)?;
	let mut date = today;
	while date.weekday() != weekday {
		date = date.pred_opt()?;
	}
	if previous {
		date = date.checked_sub_signed(Duration::days(7))?;
	}
	Some(Span::day(date))
}

fn weekday_name(name: &str) -> Option<Weekday> {
	Some(match name {
		"monday" | "mon" => Weekday::Mon,
		"tuesday" | "tue" => Weekday::Tue,
		"wednesday" | "wed" => Weekday::Wed,
		"thursday" | "thu" => Weekday::Thu,
		"friday" | "fri" => Weekday::Fri,
		"saturday" | "sat" => Weekday::Sat,
		"sunday" | "sun" => Weekday::Sun,
		_ => return None,
	})
}

fn relative(key: &str, today: NaiveDate) -> Result<Option<Span>, String> {
	let trimmed = key
		.strip_prefix("last ")
		.or_else(|| key.strip_prefix("past "))
		.unwrap_or(key);
	let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
	if digits.is_empty() {
		return Ok(None);
	}
	let n: i64 = digits.parse().map_err(|_| format!("'{key}' is not a range"))?;
	if n <= 0 {
		return Err("a range has to be at least 1".into());
	}
	let unit = trimmed[digits.len()..].trim();
	let span = match unit {
		"d" | "day" | "days" => Span::ending(today, n),
		"w" | "week" | "weeks" => Span::ending(today, n.saturating_mul(7)),
		"m" | "month" | "months" => months_back(today, n),
		"y" | "year" | "years" => months_back(today, n.saturating_mul(12)),
		_ => return Ok(None),
	};
	if span.len_days() > 366 * 20 {
		return Err("that range is longer than 20 years".into());
	}
	Ok(Some(span))
}

fn months_back(today: NaiveDate, months: i64) -> Span {
	let months = months.max(0) as u32;
	Span::new(sub_months(today, months), today)
}

fn sub_months(date: NaiveDate, months: u32) -> NaiveDate {
	if months == 0 {
		return date;
	}
	if let Some(found) = date.checked_sub_months(Months::new(months)) {
		return found;
	}
	let mut cursor = date;
	for _ in 0..3 {
		cursor = match cursor.pred_opt() {
			Some(prev) => prev,
			None => return date,
		};
		if let Some(found) = cursor.checked_sub_months(Months::new(months)) {
			return found;
		}
	}
	date
}

fn calendar_token(raw: &str, today: NaiveDate) -> Result<Option<Span>, String> {
	if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
		return Ok(Some(Span::day(date)));
	}
	if let Some((year, month)) = year_month(raw) {
		if !(1..=12).contains(&month) {
			return Err(format!("'{raw}' is not a month (use YYYY-MM)"));
		}
		let start = NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(|| format!("'{raw}' is not a month"))?;
		let end = last_day_of_month(start);
		let end = if start <= today && end > today { today } else { end };
		return Ok(Some(Span::new(start, end)));
	}
	if raw.len() == 4 && raw.chars().all(|c| c.is_ascii_digit()) {
		let year: i32 = raw.parse().map_err(|_| format!("'{raw}' is not a year"))?;
		let start = NaiveDate::from_ymd_opt(year, 1, 1).ok_or_else(|| format!("'{raw}' is not a year"))?;
		let end = NaiveDate::from_ymd_opt(year, 12, 31).ok_or_else(|| format!("'{raw}' is not a year"))?;
		let end = if start <= today && end > today { today } else { end };
		return Ok(Some(Span::new(start, end)));
	}
	Ok(None)
}

fn year_month(raw: &str) -> Option<(i32, u32)> {
	let (year, month) = raw.split_once('-')?;
	if year.len() != 4 || month.len() != 2 {
		return None;
	}
	if !year.chars().all(|c| c.is_ascii_digit()) || !month.chars().all(|c| c.is_ascii_digit()) {
		return None;
	}
	Some((year.parse().ok()?, month.parse().ok()?))
}

fn last_day_of_month(first: NaiveDate) -> NaiveDate {
	let next = if first.month() == 12 {
		NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
	} else {
		NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
	};
	next.and_then(|date| date.pred_opt()).unwrap_or(first)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn today() -> NaiveDate {
		NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
	}

	#[test]
	fn yesterday_week_and_month() {
		let today = today();
		assert_eq!(
			parse_when("yesterday", today).unwrap(),
			Span::day(NaiveDate::from_ymd_opt(2026, 10, 19).unwrap())
		);
		assert_eq!(
			parse_when("week", today).unwrap(),
			Span::new(NaiveDate::from_ymd_opt(2026, 10, 19).unwrap(), today)
		);
		assert_eq!(
			parse_when("last week", today).unwrap(),
			Span::new(
				NaiveDate::from_ymd_opt(2026, 10, 12).unwrap(),
				NaiveDate::from_ymd_opt(2026, 10, 18).unwrap()
			)
		);
		assert_eq!(
			parse_when("month", today).unwrap(),
			Span::new(NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), today)
		);
		assert_eq!(parse_when("7d", today).unwrap().len_days(), 7);
		assert_eq!(
			parse_when("monday", today).unwrap().start,
			NaiveDate::from_ymd_opt(2026, 10, 19).unwrap()
		);
	}

	#[test]
	fn explicit_range_and_widen() {
		let span = parse_when("2026-10-01..2026-10-03", today()).unwrap();
		assert_eq!(span.len_days(), 3);
		let day = Span::day(today());
		assert_eq!(day.widen().len_days(), 2);
		assert_eq!(day.shorten().len_days(), 1);
		let back = day.pan(-1, false, today());
		assert_eq!(back.end, NaiveDate::from_ymd_opt(2026, 10, 19).unwrap());
	}
}
