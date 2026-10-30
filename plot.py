#!/usr/bin/env python3
"""Draw a mood chart from the JSON spec written by the mood command."""

import json
import sys
from datetime import datetime
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.dates as mdates
import matplotlib.pyplot as plt
import numpy as np

SERIES_COLORS = ["#3b6ea5", "#d97706", "#0f766e", "#be123c", "#7c3aed", "#65a30d", "#0369a1", "#b45309"]


def main():
	spec = json.loads(Path(sys.argv[1]).read_text())
	out = Path(spec["out"])
	out.parent.mkdir(parents=True, exist_ok=True)
	fig, ax = plt.subplots(figsize=figsize(spec), layout="constrained")
	style_axes(ax)
	kind = spec.get("kind", "line")
	if kind == "heatmap":
		draw_heatmap(ax, spec)
	elif kind == "histogram":
		draw_bars(ax, spec, histogram=True)
	elif kind == "bar":
		draw_bars(ax, spec, histogram=False)
	elif kind == "scatter":
		draw_points(ax, spec)
	elif kind == "step":
		draw_lines(ax, spec, step=True, area=False)
	elif kind == "area":
		draw_lines(ax, spec, step=False, area=True)
	else:
		draw_lines(ax, spec, step=False, area=False)
	if kind != "heatmap":
		ax.set_xlabel(spec.get("x_label") or "")
		ax.set_ylabel(spec.get("y_label") or "")
		apply_y(ax, spec)
		if not any_point(spec.get("series", [])):
			ax.text(0.5, 0.5, "no logs in this range", transform=ax.transAxes, ha="center", va="center", color="#666")
	ax.set_title(spec.get("title", "Mood"), loc="left", fontsize=13, fontweight="bold", pad=14)
	ax.text(
		0.0, 1.01, spec.get("subtitle", ""), transform=ax.transAxes, ha="left", va="bottom", fontsize=8.5, color="#444"
	)
	fig.savefig(out, dpi=140, facecolor="white")
	plt.close(fig)


def figsize(spec):
	if spec.get("kind") == "heatmap":
		rows = max(1, len(spec.get("heatmap", {}).get("rows", [])))
		return (11.4, min(18.0, max(4.4, 0.28 * rows + 1.8)))
	return (11.4, 5.9)


def style_axes(ax):
	ax.spines["top"].set_visible(False)
	ax.spines["right"].set_visible(False)
	ax.yaxis.grid(True, color="#d0d0d0", lw=0.6)
	ax.set_axisbelow(True)
	ax.tick_params(labelsize=8)


def any_point(series):
	return any(value is not None for item in series for value in item.get("y", []))


def as_floats(values):
	return np.array([np.nan if value is None else float(value) for value in values], dtype=float)


def xs_of(spec):
	categories = spec.get("categories", [])
	if spec.get("x_kind") == "time":
		return [datetime.fromisoformat(value) for value in categories]
	return np.arange(len(categories))


def series_style(item, index, count):
	if item.get("emphasize"):
		return "#4b5563", 1.4, 1.0, 2
	alpha = 0.45 if count > 8 else 1.0
	width = 1.3 if count > 8 else 1.9
	return SERIES_COLORS[index % len(SERIES_COLORS)], width, alpha, 4


def add_legend(ax):
	handles, labels = ax.get_legend_handles_labels()
	if labels:
		ax.legend(handles, labels, frameon=False, fontsize=8, loc="upper left")


def series_label(item, count):
	if item.get("emphasize"):
		return item.get("name") or "mean"
	if count == 1 or count > 10:
		return None
	return item.get("name")


def draw_lines(ax, spec, step, area):
	categories = spec.get("categories", [])
	series = spec.get("series", [])
	xs = xs_of(spec)
	for index, item in enumerate(series):
		ys = as_floats(item.get("y", []))
		if len(ys) != len(categories):
			continue
		px, py = observed(xs, ys)
		if len(py) == 0:
			continue
		color, width, alpha, zorder = series_style(item, index, len(series))
		label = series_label(item, len(series))
		if area:
			ax.fill_between(px, py, y2=0, color=color, alpha=0.16, linewidth=0, zorder=zorder - 1)
		plot = ax.step if step else ax.plot
		kwargs = {"where": "post"} if step else {}
		marker = None if item.get("emphasize") or len(py) > 48 else "o"
		linestyle = "--" if item.get("emphasize") else "-"
		plot(
			px,
			py,
			color=color,
			lw=width,
			alpha=alpha,
			label=label,
			zorder=zorder,
			marker=marker,
			ms=3.5,
			linestyle=linestyle,
			**kwargs,
		)
	finish_xy(ax, spec)
	mean_line(ax, spec)
	add_legend(ax)


def observed(xs, ys):
	mask = ~np.isnan(ys)
	if isinstance(xs, np.ndarray):
		return xs[mask], ys[mask]
	return [x for x, keep in zip(xs, mask) if keep], ys[mask]


def draw_points(ax, spec):
	series = spec.get("series", [])
	xs = xs_of(spec)
	for index, item in enumerate(series):
		ys = as_floats(item.get("y", []))
		if len(ys) != len(xs):
			continue
		color, _, alpha, zorder = series_style(item, index, len(series))
		mask = ~np.isnan(ys)
		chosen = [x for x, keep in zip(xs, mask) if keep] if spec.get("x_kind") == "time" else np.asarray(xs)[mask]
		ax.scatter(
			chosen, ys[mask], s=28, color=color, alpha=alpha, label=series_label(item, len(series)), zorder=zorder
		)
	finish_xy(ax, spec)
	mean_line(ax, spec)
	add_legend(ax)


def draw_bars(ax, spec, histogram):
	categories = spec.get("categories", [])
	series = [item for item in spec.get("series", []) if item.get("y")]
	if not series:
		finish_category(ax, categories)
		return
	count = len(series)
	# Minute slots are narrower than a pixel.
	width = max(0.82 / max(count, 1), len(categories) / 200)
	for index, item in enumerate(series):
		ys = item.get("y", [])
		offset = (index - (count - 1) / 2) * width
		positions = []
		heights = []
		colors = []
		for slot, value in enumerate(ys):
			if value is None:
				continue
			positions.append(slot + offset)
			heights.append(value)
			if histogram:
				colors.append(mood_color(slot))
			elif count == 1:
				colors.append(mood_color(value))
			else:
				colors.append(series_style(item, index, count)[0])
		if not positions:
			continue
		ax.bar(
			positions,
			heights,
			width=width * 0.92,
			color=colors,
			label=series_label(item, count),
			zorder=3,
		)
	finish_category(ax, categories)
	if not histogram:
		mean_line(ax, spec)
		ax.set_xlim(-0.6, max(len(categories) - 0.4, 0.6))
	add_legend(ax)


def draw_heatmap(ax, spec):
	heat = spec.get("heatmap", {})
	rows = heat.get("rows") or ["—"]
	cols = heat.get("cols") or [f"{hour:02d}" for hour in range(24)]
	grid = heat.get("z") or []
	if not grid:
		grid = [[None] * len(cols)]
	values = np.array([[np.nan if value is None else float(value) for value in row] for row in grid], dtype=float)
	cmap = plt.cm.RdYlGn.copy()
	cmap.set_bad("#ececec")
	image = ax.imshow(values, aspect="auto", cmap=cmap, vmin=0, vmax=10, interpolation="nearest")
	step = 2 if len(cols) > 12 else 1
	ax.set_xticks(range(0, len(cols), step))
	ax.set_xticklabels([cols[index] for index in range(0, len(cols), step)])
	if len(rows) <= 40:
		shown = range(len(rows))
	else:
		stride = max(1, len(rows) // 24)
		shown = range(0, len(rows), stride)
	ax.set_yticks(list(shown))
	ax.set_yticklabels([rows[index] for index in shown], fontsize=8)
	ax.set_xlabel("Hour")
	ax.figure.colorbar(image, ax=ax, fraction=0.046, pad=0.02, label="Mood (0–10)")  # noqa: RUF001 -- real en dash


def finish_xy(ax, spec):
	if spec.get("x_kind") == "time":
		finish_time(ax, spec)
	else:
		finish_category(ax, spec.get("categories", []))


def finish_category(ax, categories):
	count = len(categories)
	if count == 0:
		return
	if count <= 16:
		indexes = list(range(count))
	elif count in (24, 1440):
		indexes = [*range(0, count, count // 8), count - 1]
	else:
		stride = max(1, count // 12)
		indexes = list(range(0, count, stride))
		if indexes[-1] != count - 1:
			indexes.append(count - 1)
	ax.set_xticks(indexes)
	rotation = 0 if count <= 14 else 35
	align = "center" if rotation == 0 else "right"
	ax.set_xticklabels([categories[index] for index in indexes], rotation=rotation, ha=align)


def finish_time(ax, spec):
	days = int(spec.get("span_days") or 1)
	if days <= 1:
		ax.xaxis.set_major_formatter(mdates.DateFormatter("%H:%M"))
		ax.xaxis.set_major_locator(mdates.HourLocator(interval=2))
	elif days <= 3:
		ax.xaxis.set_major_formatter(mdates.DateFormatter("%d %H:%M"))
		ax.xaxis.set_major_locator(mdates.HourLocator(interval=6))
	elif days <= 21:
		ax.xaxis.set_major_formatter(mdates.DateFormatter("%m-%d"))
		ax.xaxis.set_major_locator(mdates.DayLocator())
	elif days <= 120:
		ax.xaxis.set_major_formatter(mdates.DateFormatter("%m-%d"))
		ax.xaxis.set_major_locator(mdates.WeekdayLocator(byweekday=mdates.MO.weekday))
	else:
		ax.xaxis.set_major_formatter(mdates.DateFormatter("%Y-%m"))
		ax.xaxis.set_major_locator(mdates.MonthLocator())
	for label in ax.get_xticklabels():
		label.set_rotation(0 if days <= 2 else 30)
		label.set_ha("center" if days <= 2 else "right")


def apply_y(ax, spec):
	if spec.get("kind") == "histogram":
		top = float(spec.get("y_max") or 1)
		ax.set_ylim(0, max(1.0, top * 1.18))
		return
	ax.set_ylim(-0.25, 10.25)
	ax.set_yticks(range(0, 11, 2))


def mean_line(ax, spec):
	mean = spec.get("mean")
	if mean is None:
		return
	ax.axhline(float(mean), color="#6b7280", lw=0.9, ls="--", zorder=1)


def mood_color(value):
	try:
		tone = max(0.0, min(1.0, float(value) / 10.0))
	except (TypeError, ValueError):
		return "#bdbdbd"
	if tone < 0.5:
		return blend("#d64545", "#e0b84a", tone / 0.5)
	return blend("#e0b84a", "#2f9e57", (tone - 0.5) / 0.5)


def blend(start, end, amount):
	a = tuple(int(start[i : i + 2], 16) for i in (1, 3, 5))
	b = tuple(int(end[i : i + 2], 16) for i in (1, 3, 5))
	mixed = tuple(round(a[i] + (b[i] - a[i]) * amount) for i in range(3))
	return "#{:02x}{:02x}{:02x}".format(*mixed)


if __name__ == "__main__":
	main()
