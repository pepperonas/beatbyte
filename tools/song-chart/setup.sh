#!/bin/sh
set -eu
cd "$(dirname "$0")"
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
cargo build --release --manifest-path adapter/Cargo.toml
if [ "${1:-}" = "--demucs" ]; then
  python3 -m venv .demucs-venv
  .demucs-venv/bin/pip install demucs
fi
.venv/bin/python -m bbchart models --with-separator
.venv/bin/python -m bbchart register
