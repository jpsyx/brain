#!/usr/bin/env bash
# Entry point for the `brain` CLI. Safe to run directly from anywhere: it
# resolves its own directory and hardcodes no machine-specific path.
#
# `brain` is a Rust binary that lives in this repo. On first run (or after the
# sources change) we build it with `cargo build --release`, then exec it,
# forwarding every argument. The binary prints a small `key=value` plan to
# stdout (`cd=…`, `claude=…`, `open=…`, `edit=…`); applying those parent-shell
# effects is the caller's job. Run directly, `./run.sh cd` just prints the plan.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BIN="$SCRIPT_DIR/target/release/brain"
MANIFEST="$SCRIPT_DIR/Cargo.toml"
SRC_DIR="$SCRIPT_DIR/src"

needs_build=0
if [[ ! -x "$BIN" || "$MANIFEST" -nt "$BIN" ]]; then
  needs_build=1
elif [[ -n "$(find "$SRC_DIR" -name '*.rs' -newer "$BIN" -print -quit 2>/dev/null)" ]]; then
  needs_build=1
fi

if (( needs_build )); then
  # Diagnostics go to stderr so stdout stays the plan and nothing else.
  echo "Building brain CLI…" >&2
  ( cd "$SCRIPT_DIR" && cargo build --release ) >&2
fi

exec "$BIN" "$@"
