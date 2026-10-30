# Contributing

## Setup

```sh
cargo build
python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
```

A binary built from the checkout draws charts with that `.venv`.

## Before a pull request

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
ruff check . && ruff format --check .
basedpyright --warnings plot.py
```

Try changes with `MOOD_DIR` pointing at a scratch directory, never at real logs.

## Releasing

Write the changes under `## [Unreleased]` in `CHANGELOG.md`, bump `version` in `Cargo.toml`, rename the heading to
`## [X.Y.Z] - HH:MM CEST DD-MM-YYYY`, commit `Release vX.Y.Z` and push a `vX.Y.Z` tag. The release workflow builds the
binaries and publishes them.
