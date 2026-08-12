#!/usr/bin/env bash
# Idempotently install Brain's agent lifecycle bridges into a selected workspace.
#
# Usage: ./scripts/install_hook.sh [brain-root]
#
# Root precedence is the explicit argument, then BRAIN_ROOT, then $HOME/brain.
# The final branch is a compatibility fallback for old single-workspace repair
# instructions; multi-workspace callers should always pass the selected root.
#
# The scripts live below the selected root so they travel with that workspace.
# Hook commands are anchored to a root variable, never to the working
# directory: a hook runs wherever the agent last changed directory to, so a
# relative path stops resolving as soon as an agent runs `cd`. Naming no
# absolute root keeps synced settings portable across machines and across
# workspace root locations.
#
# Both bridges:
#   - SessionStart: records which frontend session the brain panel drives (resume).
#   - Turn complete: captures the final assistant message for authenticated receiver jobs
#     (no-op unless $BRAIN_RESPONSE_DIR is set).
#
# Safe to re-run. Bails (non-zero) if jq is missing — we lean on jq to merge so
# we don't corrupt the user's settings.

set -euo pipefail

script_dir="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
repo_session="${script_dir}/agent_session_start_hook.py"
repo_stop="${script_dir}/agent_turn_complete_hook.py"
repo_legacy_session="${script_dir}/claude_session_start_hook.py"
repo_legacy_stop="${script_dir}/claude_stop_hook.py"
repo_opencode_plugin="${script_dir}/opencode_brain_plugin.js"

if [[ $# -gt 1 ]]; then
  echo "usage: $0 [brain-root]" >&2
  exit 2
fi

selected_root="${1:-${BRAIN_ROOT:-}}"
if [[ -z "$selected_root" ]]; then
  selected_root="${HOME}/brain"
fi
case "$selected_root" in
  "~") selected_root="$HOME" ;;
  "~/"*) selected_root="${HOME}/${selected_root#\~/}" ;;
esac

hook_dir="${selected_root}/.claude/brain-hooks"
settings_dir="${selected_root}/.claude"
settings_path="${settings_dir}/settings.json"
codex_settings_dir="${HOME}/.codex"
codex_settings_path="${codex_settings_dir}/hooks.json"

# Anchored to Claude's own project root: hooks run in the session's current
# working directory, which the agent's `cd` moves, so a relative path breaks.
# Naming no absolute root keeps the synced settings file machine-portable.
sess_cmd='python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_session_start_hook.py"'
stop_cmd='python3 "${CLAUDE_PROJECT_DIR:-${BRAIN_ROOT:-$HOME/brain}}/.claude/brain-hooks/agent_turn_complete_hook.py"'
codex_sess_cmd='python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_session_start_hook.py"'
codex_stop_cmd='python3 "${BRAIN_ROOT:-$HOME/brain}/.claude/brain-hooks/agent_turn_complete_hook.py"'

if ! command -v jq >/dev/null 2>&1; then
  echo "install_hook.sh: jq is required (brew install jq)" >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  echo "install_hook.sh: python3 is required" >&2
  exit 1
fi
for f in "$repo_session" "$repo_stop" "$repo_legacy_session" "$repo_legacy_stop" "$repo_opencode_plugin"; do
  [[ -f "$f" ]] || { echo "install_hook.sh: hook script missing at $f" >&2; exit 1; }
done

install_static_file() {
  python3 - "$selected_root" "$1" "$2" "$3" <<'PY'
import os
from pathlib import Path
import sys
import tempfile

root = Path(sys.argv[1]).expanduser().resolve(strict=False)
source = Path(sys.argv[2]).resolve(strict=True)
destination = Path(sys.argv[3]).expanduser()
mode = int(sys.argv[4], 8)
resolved_destination = destination.resolve(strict=False)
try:
    resolved_destination.relative_to(root)
except ValueError:
    print(
        f"install_hook.sh: lifecycle artifact {destination} resolves outside workspace {root}",
        file=sys.stderr,
    )
    raise SystemExit(1)

resolved_destination.parent.mkdir(parents=True, exist_ok=True)
descriptor, temporary = tempfile.mkstemp(
    prefix=f".{resolved_destination.name}.tmp-",
    dir=resolved_destination.parent,
)
try:
    os.fchmod(descriptor, mode)
    with os.fdopen(descriptor, "wb") as output:
        output.write(source.read_bytes())
        output.flush()
        os.fsync(output.fileno())
    os.replace(temporary, resolved_destination)
    directory = os.open(resolved_destination.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)
except BaseException:
    try:
        os.close(descriptor)
    except OSError:
        pass
    try:
        os.unlink(temporary)
    except FileNotFoundError:
        pass
    raise
PY
}

# Deploy the scripts under the synced brain dir without following a workspace
# artifact symlink outside the selected root.
install_static_file "$repo_session" "$hook_dir/agent_session_start_hook.py" 0755
install_static_file "$repo_stop" "$hook_dir/agent_turn_complete_hook.py" 0755
install_static_file "$repo_legacy_session" "$hook_dir/claude_session_start_hook.py" 0755
install_static_file "$repo_legacy_stop" "$hook_dir/claude_stop_hook.py" 0755

opencode_plugin_dir="${selected_root}/.opencode/plugins"
install_static_file "$repo_opencode_plugin" "$opencode_plugin_dir/brain.js" 0644

install_hook_settings() {
  local target_path="$1"
  local session_command="$2"
  local stop_command="$3"
  local tmp

  mkdir -p "$(dirname "$target_path")"
  [[ -f "$target_path" ]] || echo "{}" > "$target_path"

  # Strip stale Brain entries by script basename, preserve unrelated settings,
  # then install each canonical command exactly once.
  tmp="$(mktemp)"
  jq --arg sess "$session_command" --arg stop "$stop_command" '
    def strip($bases):
      map(.hooks |= map(select((.command // "" | rtrimstr("\"") | rtrimstr("\u0027")) as $command | ($bases | any(. as $base | $command | endswith($base))) | not)))
      | map(select((.hooks | length) > 0));
    .hooks //= {}
    | .hooks.SessionStart = ((.hooks.SessionStart // []) | strip(["claude_session_start_hook.py", "agent_session_start_hook.py"]))
        + [{"hooks": [{"type": "command", "command": $sess}]}]
    | .hooks.Stop = ((.hooks.Stop // []) | strip(["claude_stop_hook.py", "agent_turn_complete_hook.py"]))
        + [{"hooks": [{"type": "command", "command": $stop}]}]
  ' "$target_path" > "$tmp"
  mv "$tmp" "$target_path"
}

install_hook_settings "$settings_path" "$sess_cmd" "$stop_cmd"
install_hook_settings "$codex_settings_path" "$codex_sess_cmd" "$codex_stop_cmd"

echo "install_hook.sh: hooks installed (root-anchored) → $hook_dir"
echo "OpenCode plugin: $opencode_plugin_dir/brain.js"
echo "settings: $settings_path"
echo "Codex settings: $codex_settings_path"
