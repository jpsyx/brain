#!/usr/bin/env bash
# Install or update the command from this checkout.
set -euo pipefail

usage() {
  printf '%s\n' 'Install or update brain as an executable command.

Usage: ./install.sh [--name <command>] [--target-source <checkout>] [-h|--help]

Options:
  --name <command>  Command filename (default: brain).
  --target-source <checkout>  Build a local Brain source with this installer.
  -h, --help        Show this help without installing anything.

Environment:
  BIN_DIR          Installation directory (default: $HOME/.local/bin).

Examples:
  ./install.sh
  BIN_DIR="$HOME/bin" ./install.sh --name brain-dev'
}

for arg in "$@"; do
  case "$arg" in -h|--help) usage; exit 0 ;; esac
done

command_name="brain"
target_source=""
while (($#)); do
  case "$1" in
    --name)
      if (($# < 2)); then usage >&2; exit 2; fi
      command_name="$2"
      shift 2
      ;;
    --target-source)
      if (($# < 2)); then usage >&2; exit 2; fi
      target_source="$2"
      [[ -n "$target_source" ]] || { usage >&2; exit 2; }
      shift 2
      ;;
    *) usage >&2; exit 2 ;;
  esac
done
case "$command_name" in
  ''|[.-]*|*..*|*[!a-zA-Z0-9_.-]*) usage >&2; exit 2 ;;
esac
if ((${#command_name} > 100)); then usage >&2; exit 2; fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

SOURCE_DIR="${target_source:-$SCRIPT_DIR}"
if [[ ! -d "$SOURCE_DIR" || ! -f "$SOURCE_DIR/Cargo.toml" ]]; then
  echo 'error: target source must be a local Brain source directory.' >&2
  exit 1
fi
SOURCE_DIR="$(cd -- "$SOURCE_DIR" && pwd)"
if ! command -v python3 >/dev/null 2>&1; then
  echo "error: 'python3' not found; install Python 3 and retry." >&2
  exit 1
fi
python3 - "$SOURCE_DIR/Cargo.toml" <<'PYTHON'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text()
package = re.search(r'^\[package\]\s*$(.*?)(?=^\[|\Z)', text, re.M | re.S)
if not package or not re.search(r'^name\s*=\s*"brain"\s*$', package[1], re.M):
    sys.exit('error: target source package must be brain')
PYTHON

# Where to install the binary. Callers may pass BIN_DIR; otherwise default to the
# conventional user bin dir. Resolved to an absolute path so the $PATH check
# below compares like with like.
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$BIN_DIR"
BIN_DIR="$(cd -- "$BIN_DIR" && pwd)"
INSTALLED_BINARY="$BIN_DIR/$command_name"
if [[ -d "$INSTALLED_BINARY" ]]; then
  printf 'Cannot replace directory: %s\n' "$INSTALLED_BINARY" >&2
  exit 1
fi

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
# An explicit destination avoids stale artifacts when CARGO_TARGET_DIR is set.
BUILD_DIR="$SOURCE_DIR/target"
(cd "$SOURCE_DIR" && cargo build --release --bin brain --target-dir "$BUILD_DIR") >&2

stage="$(mktemp -d "$BIN_DIR/.brain-install.XXXXXXXX")"
chmod 0700 "$stage"
trap 'rm -rf -- "$stage"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
install -m 0755 "$BUILD_DIR/release/brain" "$stage/target"
if ! BUILT_VERSION="$(brain_version "$stage/target")"; then
  echo "error: built binary did not report a supported Brain version." >&2
  exit 1
fi
sha256() {
  python3 - "$1" <<'PYTHON'
import hashlib, sys
with open(sys.argv[1], 'rb') as source:
    digest = hashlib.sha256()
    for chunk in iter(lambda: source.read(1024 * 1024), b''):
        digest.update(chunk)
print(digest.hexdigest())
PYTHON
}
expected_digest="$(sha256 "$stage/target")"
[[ "$expected_digest" =~ ^[0-9a-f]{64}$ ]] || { echo 'error: invalid target digest.' >&2; exit 1; }
coordinator_version="$BUILT_VERSION"
downgrade=false
if [[ -n "$INSTALLED_VERSION" ]] && version_is_greater "$INSTALLED_VERSION" "$BUILT_VERSION"; then
  downgrade=true
  coordinator_version="$INSTALLED_VERSION"
  install -m 0755 "$INSTALLED_BINARY" "$stage/helper"
else
  install -m 0755 "$stage/target" "$stage/helper"
fi
refresh=false
if ! version_is_greater 0.102.25 "$coordinator_version"; then refresh=true; fi
if [[ -n "$target_source" ]] && version_is_greater 0.102.25 "$BUILT_VERSION" && { ! $downgrade || ! $refresh; }; then
  echo 'error: legacy target source requires an installed migration-capable Brain; use the current installer first.' >&2
  exit 1
fi
refresh_skills() {
  "$stage/helper" __refresh-workspace-skills --target-binary "$1" \
    --target-version "$BUILT_VERSION" --phase "$2" \
    --expected-target-sha256 "$expected_digest"
}
if $refresh; then refresh_skills "$stage/target" preflight; fi
[[ "$(sha256 "$stage/target")" = "$expected_digest" ]] || { echo 'error: frozen target changed; installation stopped.' >&2; exit 1; }
if $downgrade; then
  echo "Migrating brain ${INSTALLED_VERSION} -> ${BUILT_VERSION} before downgrade…" >&2
  if $refresh; then
    "$stage/helper" __migrate --from-version "$INSTALLED_VERSION" \
      --to-version "$BUILT_VERSION" --installer-refresh-protocol 1
  else
    "$stage/helper" __migrate --from-version "$INSTALLED_VERSION" --to-version "$BUILT_VERSION"
  fi
fi
[[ "$(sha256 "$stage/target")" = "$expected_digest" ]] || { echo 'error: frozen target changed; installation stopped.' >&2; exit 1; }
mv -f -- "$stage/target" "$INSTALLED_BINARY"
[[ -f "$INSTALLED_BINARY" && ! -L "$INSTALLED_BINARY" && -x "$INSTALLED_BINARY" ]] || { echo 'error: installed target is not a regular executable.' >&2; exit 1; }
[[ "$(sha256 "$INSTALLED_BINARY")" = "$expected_digest" ]] || { echo 'error: installed target digest changed.' >&2; exit 1; }
[[ "$(brain_version "$INSTALLED_BINARY")" = "$BUILT_VERSION" ]] || { echo 'error: installed target version changed.' >&2; exit 1; }
if ! $downgrade; then
  MIGRATION_FROM="${INSTALLED_VERSION:-$BUILT_VERSION}"
  echo "Migrating brain ${MIGRATION_FROM} -> ${BUILT_VERSION}…" >&2
  "$stage/helper" __migrate --from-version "$MIGRATION_FROM" --to-version "$BUILT_VERSION"
fi
if $refresh; then refresh_skills "$INSTALLED_BINARY" apply; fi

echo "installed $command_name -> $INSTALLED_BINARY"

# A binary nobody can invoke is not an install. Say so, with the fix.
case ":${PATH}:" in
  *":$BIN_DIR:"*) ;;
  *)
    echo >&2
    echo "note: $BIN_DIR is not on your \$PATH, so \`$command_name\` won't be found yet." >&2
    echo "      Add it to your shell startup file, e.g.:" >&2
    echo "        echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc   # or ~/.bashrc" >&2
    ;;
esac
