# Changelog

## [Unreleased]

First release.

### Added

- Logging a 0–10 mood with optional comma-separated reasons, backdated with `at`.
- `mood stats`, a terminal view grouped by hour, exact time, timeline, day, week, month, year or weekday, as line,
  step, scatter, bar, area, heatmap or histogram charts.
- `mood graph`, the same charts as PNG files through matplotlib; bare, it asks what to draw.
- `mood reasons`, `mood undo`, `mood path`, `mood doctor` and `mood version`.
- One JSON-lines log per computer and a `summary.json` rebuilt from them.
- Logs in `$MOOD_DIR`, `~/syncthing/mood` when it exists, or `$XDG_DATA_HOME/mood`.
- Prebuilt binaries for x86_64 Linux and aarch64 Android (Termux).
