#!/usr/bin/env bash
# Build the `brain` CLI and install it onto $PATH as a real binary.
#
# Re-running this rebuilds and OVERWRITES the installed binary in place, so it
# doubles as the updater and never leaves a second copy behind. Safe to run
# directly from a clone; it resolves its own directory and hardcodes no
# machine-specific path.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# Where to install the binary. Callers may pass BIN_DIR; otherwise default to the
# conventional user bin dir, which is already on $PATH.
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$BIN_DIR"

echo "Building brain (release)…" >&2
( cd "$SCRIPT_DIR" && cargo build --release ) >&2

# Install to a fixed filename so each run overwrites the previous binary.
install -m 0755 "$SCRIPT_DIR/target/release/brain" "$BIN_DIR/brain"

echo "installed brain -> $BIN_DIR/brain"
