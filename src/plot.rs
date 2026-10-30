use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::agg::{self, View};
use crate::data;
use crate::entry::Entry;
use crate::kind::{Bucket, ChartKind};
use crate::range::Span;

pub fn render(entries: &[Entry], span: Span, bucket: Bucket, kind: ChartKind, out: PathBuf) -> Result<(), String> {
	if let Some(parent) = out.parent() {
		std::fs::create_dir_all(parent).map_err(|err| format!("could not create {}: {err}", parent.display()))?;
	}
	let view = agg::build_view(entries, span, bucket);
	let spec = spec_json(&view, span, bucket, kind, &out);
	let python = python_cmd();
	if !matplotlib_ok(&python) {
		return Err(missing_matplotlib(&python));
	}
	let nanos = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|time| time.subsec_nanos())
		.unwrap_or_default();
	let dir = std::env::temp_dir().join(format!("mood-plot-{}-{nanos}", std::process::id()));
	// Private, so the script can't be swapped before it runs.
	std::fs::DirBuilder::new()
		.mode(0o700)
		.create(&dir)
		.map_err(|err| format!("could not create {}: {err}", dir.display()))?;
	let result = run_script(&python, &dir, &spec);
	let _ = std::fs::remove_dir_all(&dir);
	result
}

fn run_script(python: &str, dir: &Path, spec: &Value) -> Result<(), String> {
	let script = dir.join("plot.py");
	let spec_path = dir.join("spec.json");
	std::fs::write(&script, include_str!("../plot.py")).map_err(|err| err.to_string())?;
	let bytes = serde_json::to_vec(spec).map_err(|err| err.to_string())?;
	std::fs::write(&spec_path, bytes).map_err(|err| err.to_string())?;
	let output = Command::new(python)
		.arg(&script)
		.arg(&spec_path)
		.output()
		.map_err(|err| format!("could not run {python}: {err}"))?;
	if !output.status.success() {
		let err = String::from_utf8_lossy(&output.stderr);
		let out = String::from_utf8_lossy(&output.stdout);
		return Err(format!("plot failed\n{err}{out}"));
	}
	Ok(())
}

pub fn output_path(requested: Option<PathBuf>, span: Span, bucket: Bucket, kind: ChartKind) -> PathBuf {
	let name = file_name(span, bucket, kind);
	match requested {
		Some(path) if is_image(&path) => path,
		Some(path) => path.join(name),
		None => data::graph_dir().join(name),
	}
}

fn file_name(span: Span, bucket: Bucket, kind: ChartKind) -> String {
	let range = if span.start == span.end {
		span.start.format("%Y-%m-%d").to_string()
	} else {
		format!("{}--{}", span.start.format("%Y-%m-%d"), span.end.format("%Y-%m-%d"))
	};
	if kind == ChartKind::Line && bucket == Bucket::HourOfDay {
		format!("mood-{range}.png")
	} else {
		format!("mood-{range}-{}-{}.png", bucket.slug(), kind.slug())
	}
}

fn is_image(path: &Path) -> bool {
	matches!(
		path.extension()
			.and_then(|ext| ext.to_str())
			.map(|ext| ext.to_ascii_lowercase())
			.as_deref(),
		Some("png" | "svg" | "pdf" | "jpg" | "jpeg" | "webp")
	)
}

pub fn python_cmd() -> String {
	if let Ok(python) = std::env::var("MOOD_PYTHON") {
		if !python.is_empty() {
			return python;
		}
	}
	// A source checkout's own .venv.
	let venv = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".venv/bin/python");
	if venv.is_file() {
		return venv.to_string_lossy().into_owned();
	}
	"python3".into()
}

pub fn missing_matplotlib(python: &str) -> String {
	format!("matplotlib is not installed for {python}\n  install it there, or set MOOD_PYTHON to a Python that has it")
}

pub fn matplotlib_ok(python: &str) -> bool {
	Command::new(python)
		.args(["-c", "import matplotlib"])
		.status()
		.map(|status| status.success())
		.unwrap_or(false)
}

fn spec_json(view: &View, span: Span, bucket: Bucket, kind: ChartKind, out: &Path) -> Value {
	let histogram = kind == ChartKind::Histogram;
	let (categories, series) = if histogram {
		let categories: Vec<String> = (0..11).map(|bin| bin.to_string()).collect();
		let y: Vec<Option<f64>> = view.hist.iter().map(|count| Some(*count as f64)).collect();
		(categories, vec![json!({"name": "logs", "y": y, "emphasize": false})])
	} else {
		(
			view.categories.clone(),
			view.series
				.iter()
				.map(|series| {
					json!({
						"name": series.name,
						"y": series.y,
						"emphasize": series.emphasize,
					})
				})
				.collect(),
		)
	};
	let y_max = if histogram {
		view.hist.iter().copied().max().unwrap_or(1).max(1) as f64
	} else {
		10.0
	};
	json!({
		"title": format!("Mood · {} · {}", kind.label(), bucket.label()),
		"subtitle": agg::subtitle(&view.summary, span),
		"kind": kind.slug(),
		"bucket": bucket.slug(),
		"x_kind": if bucket == Bucket::Timeline && !histogram { "time" } else { "category" },
		"x_label": x_label(bucket, kind),
		"y_label": if histogram { "Logs" } else { "Mood (0–10)" },
		"y_min": 0,
		"y_max": y_max,
		"span_days": span.len_days(),
		"mean": if view.summary.n == 0 { Value::Null } else { json!(view.summary.mean) },
		"categories": categories,
		"series": series,
		"heatmap": {
			"rows": view.heatmap_rows,
			"cols": (0..24).map(|hour| format!("{hour:02}")).collect::<Vec<_>>(),
			"z": view.heatmap,
		},
		"out": out.to_string_lossy(),
	})
}

fn x_label(bucket: Bucket, kind: ChartKind) -> &'static str {
	if kind == ChartKind::Histogram {
		return "Score";
	}
	match bucket {
		Bucket::HourOfDay => "Hour",
		Bucket::Exact => "Time of day",
		Bucket::Timeline => "Time",
		Bucket::Day => "Day",
		Bucket::Week => "Week",
		Bucket::Month => "Month",
		Bucket::Year => "Year",
		Bucket::Weekday => "Weekday",
	}
}
