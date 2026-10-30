# Mood Tracker

[![CI](https://github.com/MattFor/mood/actions/workflows/ci.yml/badge.svg)](https://github.com/MattFor/mood/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/MattFor/mood)](https://github.com/MattFor/mood/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Log mood scores and review them in the terminal or as a chart.  
Originally built to keep track of my own emotional state. I hope you find it just as useful!

## Features

- A 0–10 score with optional reasons in one command: `mood 6 work, bad sleep`.
- Backdating: `mood 4 at yesterday 21:00`, `at friday 9`, `at 19.10 20:00`.
- `mood stats`: a terminal view of any span, grouped by hour, exact time, timeline, day, week, month, year or
  weekday, drawn as line, step, scatter, bar, area, heatmap or histogram.
- `mood graph`: the same charts as PNG files.
- `mood reasons`: how each reason compares with your average.
- One log file per computer, so a sync tool such as Syncthing never has to merge one.

## Requirements

- Linux or another Unix. Prebuilt binaries: x86_64 Linux (static) and aarch64 Android (Termux, Android 9+).
- `mood graph` only: Python 3.9+ with matplotlib.

## Install

```sh
curl -L https://github.com/MattFor/mood/releases/latest/download/mood-x86_64-unknown-linux-musl.tar.gz | tar -xz mood
install -m 755 mood ~/.local/bin/
```

In Termux, use `mood-aarch64-linux-android.tar.gz`. `SHA256SUMS` in the release lists every file's checksum.

From source, with Rust 1.85 or newer:

```sh
cargo install --git https://github.com/MattFor/mood
```

For charts, `python3 -m pip install matplotlib`, or set `MOOD_PYTHON` to a Python that has it.

## Usage

```sh
mood                     # today and the last 7 days
mood 7 work, sleep       # log a 7 with two reasons
mood 4 at yesterday 21:00
mood stats week          # interactive view; mood stats --help lists the keys
mood graph last month    # a PNG; bare mood graph asks what to draw
mood reasons             # last 30 days by reason
mood undo                # remove this computer's last log
mood doctor              # data directory, log count, matplotlib
```

`mood help` shows every command, time range and key.

## Data format

Logs live in the first of:

1. `$MOOD_DIR`
2. `~/syncthing/mood`, if that directory exists
3. `$XDG_DATA_HOME/mood`, else `~/.local/share/mood`

Each computer appends to its own `<hostname>.jsonl` (`MOOD_HOST` overrides the name), one JSON object per line:

```json
{
  "t": "2026-10-20T14:05:00+02:00",
  "mood": 6.5,
  "reasons": ["work", "bad sleep"],
  "host": "desktop"
}
```

`summary.json` is rebuilt from the logs, `graphs/` holds charts and `FORMAT.txt` describes all of it.

## Building

```sh
cargo build --release
cargo test
```

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT, see [LICENSE](LICENSE). By [MattFor](https://github.com/MattFor).
