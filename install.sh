#!/usr/bin/env bash
# Build the `brain` CLI and install it onto $PATH as a real binary.
#
# Re-running this rebuilds and OVERWRITES the installed binary in place, so it
# doubles as the updater and never leaves a second copy behind. Safe to run
# directly from a clone; it resolves its own directory and hardcodes no
# machine-specific path.
set -euo pipefail

usage() {
  cat <<'EOF'
install.sh — build brain and install it onto $PATH as a real binary.

Usage:
  ./install.sh [--help]

Options:
  -h, --help   Print this help and exit.

Environment:
  BIN_DIR      Directory to install the `brain` binary into. Created if
               missing. Default: $HOME/.local/bin.

Examples:
  ./install.sh                      # install to ~/.local/bin/brain
  BIN_DIR=/usr/local/bin ./install.sh   # install elsewhere on $PATH
  git pull && ./install.sh          # update an existing install in place
EOF
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  "") ;;
  *)
    usage >&2
    exit 1
    ;;
esac

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# Where to install the binary. Callers may pass BIN_DIR; otherwise default to the
# conventional user bin dir. Resolved to an absolute path so the $PATH check
# below compares like with like.
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$BIN_DIR"
BIN_DIR="$(cd -- "$BIN_DIR" && pwd)"

# brain builds from source, so a Rust toolchain is the one prerequisite. Say so
# plainly rather than letting the build die on "cargo: command not found".
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: 'cargo' not found — brain builds from source and needs a Rust toolchain." >&2
  echo "       Install one from https://rustup.rs, then re-run this script." >&2
  exit 1
fi

echo "Building brain (release)…" >&2
(cd "$SCRIPT_DIR" && cargo build --release) >&2

# Install to a fixed filename so each run overwrites the previous binary.
install -m 0755 "$SCRIPT_DIR/target/release/brain" "$BIN_DIR/brain"

echo "installed brain -> $BIN_DIR/brain"

# A binary nobody can invoke is not an install. Say so, with the fix.
case ":${PATH}:" in
  *":$BIN_DIR:"*) ;;
  *)
    echo >&2
    echo "note: $BIN_DIR is not on your \$PATH, so \`brain\` won't be found yet." >&2
    echo "      Add it to your shell startup file, e.g.:" >&2
    echo "        echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc   # or ~/.bashrc" >&2
    ;;
esac
