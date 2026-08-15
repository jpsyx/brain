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
install.sh: build brain and install it onto $PATH as a real binary.

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
INSTALLED_BINARY="$BIN_DIR/brain"

brain_version() {
  local binary="$1"
  local output
  output="$("$binary" --version 2>/dev/null)" || return 1
  [[ "$output" =~ ^brain[[:space:]]+([0-9]+\.[0-9]+\.[0-9]+)([-+][^[:space:]]+)?$ ]] || return 1
  printf '%s\n' "${BASH_REMATCH[1]}"
}

version_is_greater() {
  local left="$1"
  local right="$2"
  local left_major left_minor left_patch
  local right_major right_minor right_patch
  IFS=. read -r left_major left_minor left_patch <<< "$left"
  IFS=. read -r right_major right_minor right_patch <<< "$right"
  ((
    left_major > right_major ||
    (left_major == right_major && left_minor > right_minor) ||
    (left_major == right_major && left_minor == right_minor && left_patch > right_patch)
  ))
}

INSTALLED_VERSION=""
if [[ -e "$INSTALLED_BINARY" ]]; then
  if [[ ! -x "$INSTALLED_BINARY" ]]; then
    echo "error: existing $INSTALLED_BINARY is not executable; cannot determine its Brain version." >&2
    exit 1
  fi
  if ! INSTALLED_VERSION="$(brain_version "$INSTALLED_BINARY")"; then
    echo "error: existing $INSTALLED_BINARY did not report a supported Brain version." >&2
    exit 1
  fi
fi

# brain builds from source, so a Rust toolchain is the one prerequisite. Say so
# plainly rather than letting the build die on "cargo: command not found".
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: 'cargo' not found; brain builds from source and needs a Rust toolchain." >&2
  echo "       Install one from https://rustup.rs, then re-run this script." >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: 'python3' not found; brain's agent lifecycle hooks need Python 3." >&2
  echo "       Install Python 3 with your system package manager, then re-run this script." >&2
  exit 1
fi

echo "Building brain (release)…" >&2
(cd "$SCRIPT_DIR" && cargo build --release) >&2

BUILT_BINARY="$SCRIPT_DIR/target/release/brain"
if ! BUILT_VERSION="$(brain_version "$BUILT_BINARY")"; then
  echo "error: built binary did not report a supported Brain version." >&2
  exit 1
fi

if [[ -n "$INSTALLED_VERSION" ]] && version_is_greater "$INSTALLED_VERSION" "$BUILT_VERSION"; then
  echo "Migrating brain ${INSTALLED_VERSION} -> ${BUILT_VERSION} before downgrade…" >&2
  "$INSTALLED_BINARY" __migrate \
    --from-version "$INSTALLED_VERSION" \
    --to-version "$BUILT_VERSION"
fi

# Install to a fixed filename so each run overwrites the previous binary.
install -m 0755 "$BUILT_BINARY" "$INSTALLED_BINARY"

if [[ -z "$INSTALLED_VERSION" ]] || ! version_is_greater "$INSTALLED_VERSION" "$BUILT_VERSION"; then
  MIGRATION_FROM="${INSTALLED_VERSION:-$BUILT_VERSION}"
  echo "Migrating brain ${MIGRATION_FROM} -> ${BUILT_VERSION}…" >&2
  "$INSTALLED_BINARY" __migrate \
    --from-version "$MIGRATION_FROM" \
    --to-version "$BUILT_VERSION"
fi

echo "installed brain -> $INSTALLED_BINARY"

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
