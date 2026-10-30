#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
	HourOfDay,
	Exact,
	Timeline,
	Day,
	Week,
	Month,
	Year,
	Weekday,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
	Line,
	Step,
	Scatter,
	Bar,
	Area,
	Heatmap,
	Histogram,
}

impl Bucket {
	pub fn label(self) -> &'static str {
		match self {
			Self::HourOfDay => "hour of day",
			Self::Exact => "exact time",
			Self::Timeline => "timeline",
			Self::Day => "day",
			Self::Week => "week",
			Self::Month => "month",
			Self::Year => "year",
			Self::Weekday => "weekday",
		}
	}

	pub fn slug(self) -> &'static str {
		match self {
			Self::HourOfDay => "hour",
			Self::Exact => "exact",
			Self::Timeline => "timeline",
			Self::Day => "day",
			Self::Week => "week",
			Self::Month => "month",
			Self::Year => "year",
			Self::Weekday => "weekday",
		}
	}

	pub fn cycle(self) -> Self {
		match self {
			Self::HourOfDay => Self::Exact,
			Self::Exact => Self::Timeline,
			Self::Timeline => Self::Day,
			Self::Day => Self::Week,
			Self::Week => Self::Month,
			Self::Month => Self::Year,
			Self::Year => Self::Weekday,
			Self::Weekday => Self::HourOfDay,
		}
	}

	pub fn parse(raw: &str) -> Result<Self, String> {
		match raw.trim().to_ascii_lowercase().as_str() {
			"h" | "hour" | "hours" | "hod" | "hour-of-day" | "hour of day" => Ok(Self::HourOfDay),
			"e" | "exact" | "minute" | "minutes" => Ok(Self::Exact),
			"t" | "time" | "timeline" | "continuous" => Ok(Self::Timeline),
			"d" | "day" | "days" => Ok(Self::Day),
			"w" | "week" | "weeks" => Ok(Self::Week),
			"m" | "month" | "months" => Ok(Self::Month),
			"y" | "year" | "years" => Ok(Self::Year),
			"u" | "weekday" | "weekdays" | "dow" => Ok(Self::Weekday),
			other => Err(format!(
				"unknown bucket '{other}'. use hour, exact, timeline, day, week, month, year, or weekday"
			)),
		}
	}
}

impl ChartKind {
	pub fn label(self) -> &'static str {
		match self {
			Self::Line => "line",
			Self::Step => "step",
			Self::Scatter => "scatter",
			Self::Bar => "bar",
			Self::Area => "area",
			Self::Heatmap => "heatmap",
			Self::Histogram => "histogram",
		}
	}

	pub fn slug(self) -> &'static str {
		self.label()
	}

	pub fn from_digit(n: u8) -> Option<Self> {
		Some(match n {
			1 => Self::Line,
			2 => Self::Step,
			3 => Self::Scatter,
			4 => Self::Bar,
			5 => Self::Area,
			6 => Self::Heatmap,
			7 => Self::Histogram,
			_ => return None,
		})
	}

	pub fn cycle(self) -> Self {
		match self {
			Self::Line => Self::Step,
			Self::Step => Self::Scatter,
			Self::Scatter => Self::Bar,
			Self::Bar => Self::Area,
			Self::Area => Self::Heatmap,
			Self::Heatmap => Self::Histogram,
			Self::Histogram => Self::Line,
		}
	}

	pub fn parse(raw: &str) -> Result<Self, String> {
		match raw.trim().to_ascii_lowercase().as_str() {
			"l" | "line" | "lines" => Ok(Self::Line),
			"step" | "steps" => Ok(Self::Step),
			"s" | "scatter" | "dot" | "dots" | "point" | "points" => Ok(Self::Scatter),
			"bar" | "bars" | "column" | "columns" => Ok(Self::Bar),
			"area" | "fill" => Ok(Self::Area),
			"heatmap" | "heat" | "map" => Ok(Self::Heatmap),
			"hist" | "histogram" | "distribution" => Ok(Self::Histogram),
			other => Err(format!(
				"unknown chart '{other}'. use line, step, scatter, bar, area, heatmap, or histogram"
			)),
		}
	}
}
