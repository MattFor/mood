use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Timelike};

use crate::entry::Entry;
use crate::kind::Bucket;
use crate::range::Span;

#[derive(Clone, Debug)]
pub struct Summary {
	pub n: usize,
	pub mean: f64,
	pub median: f64,
	pub min: f64,
	pub max: f64,
	pub sd: f64,
}

impl Summary {
	pub fn from_moods(moods: &[f64]) -> Self {
		if moods.is_empty() {
			return Self {
				n: 0,
				mean: 0.0,
				median: 0.0,
				min: 0.0,
				max: 0.0,
				sd: 0.0,
			};
		}
		let n = moods.len();
		let mean = moods.iter().sum::<f64>() / n as f64;
		let min = moods.iter().copied().fold(f64::INFINITY, f64::min);
		let max = moods.iter().copied().fold(f64::NEG_INFINITY, f64::max);
		let mut ordered = moods.to_vec();
		ordered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		let median = if n % 2 == 1 {
			ordered[n / 2]
		} else {
			(ordered[n / 2 - 1] + ordered[n / 2]) / 2.0
		};
		let sd = if n < 2 {
			0.0
		} else {
			let var = moods.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / (n as f64 - 1.0);
			var.sqrt()
		};
		Self {
			n,
			mean,
			median,
			min,
			max,
			sd,
		}
	}
}

#[derive(Clone, Debug)]
pub struct Series {
	pub name: String,
	pub y: Vec<Option<f64>>,
	pub emphasize: bool,
}

#[derive(Clone, Debug)]
pub struct Row {
	pub label: String,
	pub mean: f64,
	pub min: f64,
	pub max: f64,
	pub n: usize,
}

#[derive(Clone, Debug)]
pub struct ReasonRow {
	pub reason: String,
	pub n: usize,
	pub mean: f64,
}

#[derive(Clone, Debug)]
pub struct View {
	pub categories: Vec<String>,
	pub series: Vec<Series>,
	pub rows: Vec<Row>,
	pub summary: Summary,
	pub heatmap_rows: Vec<String>,
	pub heatmap: Vec<Vec<Option<f64>>>,
	pub hist: [u32; 11],
	pub reasons: Vec<ReasonRow>,
	pub logs: Vec<Entry>,
}

pub fn fmt_num(value: f64) -> String {
	if !value.is_finite() {
		return "-".into();
	}
	let rounded = (value * 10.0).round() / 10.0;
	if (rounded - rounded.round()).abs() < 0.05 {
		format!("{}", rounded.round() as i64)
	} else {
		format!("{rounded:.1}")
	}
}

pub fn subtitle(summary: &Summary, span: Span) -> String {
	if summary.n == 0 {
		format!("{}    no logs", span.label())
	} else {
		format!(
			"{}    mean {}   median {}   min {}   max {}   sd {}   n {}",
			span.label(),
			fmt_num(summary.mean),
			fmt_num(summary.median),
			fmt_num(summary.min),
			fmt_num(summary.max),
			fmt_num(summary.sd),
			summary.n
		)
	}
}

pub fn summary_between(entries: &[Entry], span: Span) -> Summary {
	let moods: Vec<f64> = entries
		.iter()
		.filter(|entry| in_span(entry, span))
		.map(|entry| entry.mood)
		.collect();
	Summary::from_moods(&moods)
}

pub fn build_view(entries: &[Entry], span: Span, bucket: Bucket) -> View {
	let mut logs: Vec<Entry> = entries.iter().filter(|entry| in_span(entry, span)).cloned().collect();
	logs.sort_by(|a, b| a.at.cmp(&b.at).then(a.host.cmp(&b.host)));
	let summary = Summary::from_moods(&logs.iter().map(|entry| entry.mood).collect::<Vec<_>>());
	let hist = histogram(&logs);
	let (heatmap_rows, heatmap) = heatmap_of(&logs, span);
	let (categories, series, rows) = match bucket {
		Bucket::HourOfDay => time_of_day(&logs, span, 60),
		Bucket::Exact => time_of_day(&logs, span, 1),
		Bucket::Timeline => timeline(&logs, span),
		Bucket::Day => by_key(&logs, span, |date| date.format("%Y-%m-%d").to_string()),
		Bucket::Week => by_key(&logs, span, week_key),
		Bucket::Month => by_key(&logs, span, |date| date.format("%Y-%m").to_string()),
		Bucket::Year => by_key(&logs, span, |date| date.format("%Y").to_string()),
		Bucket::Weekday => by_weekday(&logs),
	};
	let reasons = reason_rows(&logs);
	View {
		categories,
		series,
		rows,
		summary,
		heatmap_rows,
		heatmap,
		hist,
		reasons,
		logs,
	}
}

pub fn reason_rows(logs: &[Entry]) -> Vec<ReasonRow> {
	let mut moods: HashMap<&str, Vec<f64>> = HashMap::new();
	for entry in logs {
		for reason in &entry.reasons {
			moods.entry(reason.as_str()).or_default().push(entry.mood);
		}
	}
	let mut rows: Vec<ReasonRow> = moods
		.into_iter()
		.map(|(reason, values)| ReasonRow {
			reason: reason.to_string(),
			n: values.len(),
			mean: values.iter().sum::<f64>() / values.len() as f64,
		})
		.collect();
	rows.sort_by(|a, b| b.n.cmp(&a.n).then(a.reason.cmp(&b.reason)));
	rows
}

fn in_span(entry: &Entry, span: Span) -> bool {
	let date = entry.at.date();
	date >= span.start && date <= span.end
}

pub fn mean_opt(values: &[f64]) -> Option<f64> {
	if values.is_empty() {
		None
	} else {
		Some(values.iter().sum::<f64>() / values.len() as f64)
	}
}

fn bucket_stats(values: &[f64]) -> Option<(f64, f64, f64, usize)> {
	if values.is_empty() {
		return None;
	}
	let min = values.iter().copied().fold(f64::INFINITY, f64::min);
	let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
	let mean = values.iter().sum::<f64>() / values.len() as f64;
	Some((mean, min, max, values.len()))
}

fn rows_from(labels: &[String], buckets: &[Vec<f64>]) -> Vec<Row> {
	labels
		.iter()
		.zip(buckets)
		.filter_map(|(label, values)| {
			let (mean, min, max, n) = bucket_stats(values)?;
			Some(Row {
				label: label.clone(),
				mean,
				min,
				max,
				n,
			})
		})
		.collect()
}

/// `minutes` per slot: 60 for hours, 1 for exact times.
fn time_of_day(logs: &[Entry], span: Span, minutes: u32) -> (Vec<String>, Vec<Series>, Vec<Row>) {
	let slots = (24 * 60 / minutes) as usize;
	let clock = |slot: usize| {
		let minute = slot * minutes as usize;
		format!("{:02}:{:02}", minute / 60, minute % 60)
	};
	let categories: Vec<String> = (0..slots)
		.map(|slot| {
			if minutes == 60 {
				format!("{slot:02}")
			} else {
				clock(slot)
			}
		})
		.collect();
	// Only days with logs get slots, so minute slots over long spans stay small.
	let mut per_day: Vec<(NaiveDate, Vec<Vec<f64>>)> = Vec::new();
	let mut per_slot: Vec<Vec<f64>> = vec![Vec::new(); slots];
	for entry in logs {
		let date = entry.at.date();
		if per_day.last().is_none_or(|(day, _)| *day != date) {
			per_day.push((date, vec![Vec::new(); slots]));
		}
		let slot = ((entry.at.hour() * 60 + entry.at.minute()) / minutes) as usize;
		if let Some((_, day)) = per_day.last_mut() {
			day[slot].push(entry.mood);
		}
		per_slot[slot].push(entry.mood);
	}
	let mut series: Vec<Series> = per_day
		.iter()
		.map(|(day, values)| Series {
			name: day_label(*day, span),
			y: values.iter().map(|values| mean_opt(values)).collect(),
			emphasize: false,
		})
		.collect();
	// A per-minute mean across days would zigzag between unrelated logs.
	if series.len() > 1 && minutes == 60 {
		series.push(Series {
			name: "mean".into(),
			y: per_slot.iter().map(|values| mean_opt(values)).collect(),
			emphasize: true,
		});
	}
	let rows = if span.len_days() == 1 {
		let labels: Vec<String> = (0..slots).map(clock).collect();
		rows_from(&labels, &per_slot)
	} else {
		let labels: Vec<String> = per_day.iter().map(|(day, _)| day_label(*day, span)).collect();
		let buckets: Vec<Vec<f64>> = per_day
			.into_iter()
			.map(|(_, values)| values.into_iter().flatten().collect())
			.collect();
		rows_from(&labels, &buckets)
	};
	(categories, series, rows)
}

fn timeline(logs: &[Entry], span: Span) -> (Vec<String>, Vec<Series>, Vec<Row>) {
	let start = span
		.start
		.and_hms_opt(0, 0, 0)
		.unwrap_or(span.start.and_hms_opt(0, 0, 0).unwrap_or_default());
	let minutes = span.len_days() * 24 * 60;
	// Finest round step that stays under 2500 points.
	let step = [1, 2, 5, 10, 15, 30, 60, 120, 180, 360, 720, 1440]
		.into_iter()
		.find(|step| minutes / step <= 2500)
		.unwrap_or((minutes + 2499) / 2500);
	let slots = ((minutes + step - 1) / step) as usize;
	let mut buckets = vec![Vec::new(); slots];
	for entry in logs {
		let minute = (entry.at - start).num_minutes();
		if minute < 0 {
			continue;
		}
		let slot = (minute / step) as usize;
		if slot < buckets.len() {
			buckets[slot].push(entry.mood);
		}
	}
	let mut categories = Vec::with_capacity(slots);
	let mut row_labels = Vec::with_capacity(slots);
	for index in 0..slots {
		let stamp = start + Duration::minutes(index as i64 * step);
		categories.push(stamp.format("%Y-%m-%dT%H:%M:%S").to_string());
		row_labels.push(slot_label(stamp, span));
	}
	let y: Vec<Option<f64>> = buckets.iter().map(|values| mean_opt(values)).collect();
	let rows = rows_from(&row_labels, &buckets);
	let series = vec![Series {
		name: "mood".into(),
		y,
		emphasize: false,
	}];
	(categories, series, rows)
}

fn slot_label(stamp: NaiveDateTime, span: Span) -> String {
	if span.len_days() <= 1 {
		stamp.format("%H:%M").to_string()
	} else if span.start.year() != span.end.year() {
		stamp.format("%Y-%m-%d %H:%M").to_string()
	} else {
		stamp.format("%m-%d %H:%M").to_string()
	}
}

fn by_key(logs: &[Entry], span: Span, key_of: impl Fn(NaiveDate) -> String) -> (Vec<String>, Vec<Series>, Vec<Row>) {
	let mut categories = Vec::new();
	let mut index = HashMap::new();
	for date in span.dates() {
		let key = key_of(date);
		if !index.contains_key(&key) {
			index.insert(key.clone(), categories.len());
			categories.push(key);
		}
	}
	let mut buckets = vec![Vec::new(); categories.len()];
	for entry in logs {
		if let Some(slot) = index.get(&key_of(entry.at.date())) {
			buckets[*slot].push(entry.mood);
		}
	}
	let y: Vec<Option<f64>> = buckets.iter().map(|values| mean_opt(values)).collect();
	let rows = rows_from(&categories, &buckets);
	(
		categories,
		vec![Series {
			name: "mood".into(),
			y,
			emphasize: false,
		}],
		rows,
	)
}

fn by_weekday(logs: &[Entry]) -> (Vec<String>, Vec<Series>, Vec<Row>) {
	let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
	let categories: Vec<String> = names.iter().map(|name| (*name).to_string()).collect();
	let mut buckets: [Vec<f64>; 7] = std::array::from_fn(|_| Vec::new());
	for entry in logs {
		buckets[entry.at.weekday().num_days_from_monday() as usize].push(entry.mood);
	}
	let owned: Vec<Vec<f64>> = buckets.into_iter().collect();
	let y: Vec<Option<f64>> = owned.iter().map(|values| mean_opt(values)).collect();
	let rows = rows_from(&categories, &owned);
	(
		categories,
		vec![Series {
			name: "mood".into(),
			y,
			emphasize: false,
		}],
		rows,
	)
}

fn week_key(date: NaiveDate) -> String {
	let week = date.iso_week();
	format!("{}-W{:02}", week.year(), week.week())
}

pub fn day_label(date: NaiveDate, span: Span) -> String {
	let name = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][date.weekday().num_days_from_monday() as usize];
	if span.start.year() != span.end.year() {
		format!("{} {name}", date.format("%Y-%m-%d"))
	} else {
		format!("{} {name}", date.format("%m-%d"))
	}
}

fn histogram(logs: &[Entry]) -> [u32; 11] {
	let mut bins = [0; 11];
	for entry in logs {
		let bin = entry.mood.round().clamp(0.0, 10.0) as usize;
		bins[bin] += 1;
	}
	bins
}

fn heatmap_of(logs: &[Entry], span: Span) -> (Vec<String>, Vec<Vec<Option<f64>>>) {
	let days = span.dates();
	let mut grid = vec![vec![Vec::new(); 24]; days.len()];
	for entry in logs {
		let index = (entry.at.date() - span.start).num_days();
		if index < 0 {
			continue;
		}
		let index = index as usize;
		let hour = entry.at.hour() as usize;
		if index < grid.len() && hour < 24 {
			grid[index][hour].push(entry.mood);
		}
	}
	let rows = days.iter().copied().map(|date| day_label(date, span)).collect();
	let values = grid
		.into_iter()
		.map(|hours| hours.iter().map(|bucket| mean_opt(bucket)).collect())
		.collect();
	(rows, values)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::entry::Entry;

	fn entry(stamp: &str, mood: f64) -> Entry {
		Entry {
			at: NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M").unwrap(),
			t: stamp.into(),
			mood,
			reasons: Vec::new(),
			host: "test".into(),
		}
	}

	#[test]
	fn reasons_count_and_average() {
		let mut logs = vec![
			entry("2026-10-20 09:00", 2.0),
			entry("2026-10-20 10:00", 4.0),
			entry("2026-10-20 11:00", 9.0),
		];
		logs[0].reasons = vec!["work".into()];
		logs[1].reasons = vec!["work".into(), "sleep".into()];
		logs[2].reasons = vec!["gym".into()];
		let rows = reason_rows(&logs);
		assert_eq!(rows[0].reason, "work");
		assert_eq!(rows[0].n, 2);
		assert!((rows[0].mean - 3.0).abs() < 1e-9);
		assert_eq!(rows.len(), 3);
	}

	#[test]
	fn hour_average_leaves_gaps() {
		let logs = vec![
			entry("2026-10-20 09:00", 8.0),
			entry("2026-10-20 09:30", 6.0),
			entry("2026-10-20 15:10", 4.0),
		];
		let span = Span::day(logs[0].at.date());
		let view = build_view(&logs, span, Bucket::HourOfDay);
		assert_eq!(view.summary.n, 3);
		assert!((view.summary.mean - 6.0).abs() < 1e-9);
		let series = &view.series[0];
		assert!(series.y[0].is_none());
		assert!((series.y[9].unwrap() - 7.0).abs() < 1e-9);
		assert!((series.y[15].unwrap() - 4.0).abs() < 1e-9);
		assert_eq!(view.hist[8], 1);
		assert_eq!(view.hist[6], 1);
		assert_eq!(view.hist[4], 1);
	}

	#[test]
	fn exact_keeps_the_minute() {
		let logs = vec![
			entry("2026-10-20 09:15", 8.0),
			entry("2026-10-20 09:45", 6.0),
			entry("2026-10-21 09:15", 2.0),
		];
		let day = build_view(&logs[..2], Span::day(logs[0].at.date()), Bucket::Exact);
		assert_eq!(day.categories.len(), 1440);
		assert_eq!(day.categories[555], "09:15");
		assert!((day.series[0].y[555].unwrap() - 8.0).abs() < 1e-9);
		assert!((day.series[0].y[585].unwrap() - 6.0).abs() < 1e-9);
		assert_eq!(
			day.rows.iter().map(|row| row.label.as_str()).collect::<Vec<_>>(),
			["09:15", "09:45"]
		);
		let two = build_view(&logs, Span::new(logs[0].at.date(), logs[2].at.date()), Bucket::Exact);
		assert_eq!(two.series.len(), 2, "no mean line at minute precision");
		assert_eq!(two.rows.len(), 2);
	}

	#[test]
	fn timeline_is_minutes_for_a_day() {
		let logs = vec![entry("2026-10-20 14:05", 6.0)];
		let view = build_view(&logs, Span::day(logs[0].at.date()), Bucket::Timeline);
		assert_eq!(view.categories.len(), 1440);
		assert!((view.series[0].y[14 * 60 + 5].unwrap() - 6.0).abs() < 1e-9);
		assert_eq!(view.rows[0].label, "14:05");
		let week = build_view(&logs, Span::ending(logs[0].at.date(), 7), Bucket::Timeline);
		assert_eq!(week.categories.len(), 7 * 24 * 12);
	}

	#[test]
	fn two_days_add_a_mean_line() {
		let logs = vec![entry("2026-10-19 10:00", 2.0), entry("2026-10-20 10:00", 8.0)];
		let span = Span::new(logs[0].at.date(), logs[1].at.date());
		let view = build_view(&logs, span, Bucket::HourOfDay);
		assert_eq!(view.series.len(), 3);
		assert!(view.series[2].emphasize);
		assert!((view.series[2].y[10].unwrap() - 5.0).abs() < 1e-9);
	}
}
