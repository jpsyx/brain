#!/usr/bin/env bash
# Idempotently install brain's Claude Code hooks into a selected workspace.
#
# Usage: ./scripts/install_hook.sh [brain-root]
#
# Root precedence is the explicit argument, then BRAIN_ROOT, then $HOME/brain.
# The final branch is a compatibility fallback for old single-workspace repair
# instructions; multi-workspace callers should always pass the selected root.
#
# The scripts live below the selected root so they travel with that workspace.
# Hook commands are project-relative because Claude runs project hooks with the
# selected workspace as its working directory. This keeps synced settings
# portable across machines and across workspace root locations.
#
# Both hooks:
#   - SessionStart: records which Claude session the brain panel drives (resume).
#   - Stop: captures the final assistant message for authenticated receiver jobs
#     (no-op unless $BRAIN_RESPONSE_DIR is set).
#
# Safe to re-run. Bails (non-zero) if jq is missing — we lean on jq to merge so
# we don't corrupt the user's settings.

set -euo pipefail

script_dir="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
repo_session="${script_dir}/claude_session_start_hook.py"
repo_stop="${script_dir}/claude_stop_hook.py"

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

# Project-relative commands remain valid when the synced workspace moves.
sess_cmd="python3 .claude/brain-hooks/claude_session_start_hook.py"
stop_cmd="python3 .claude/brain-hooks/claude_stop_hook.py"

if ! command -v jq >/dev/null 2>&1; then
  echo "install_hook.sh: jq is required (brew install jq)" >&2
  exit 1
fi
for f in "$repo_session" "$repo_stop"; do
  [[ -f "$f" ]] || { echo "install_hook.sh: hook script missing at $f" >&2; exit 1; }
done

# Deploy the scripts under the synced brain dir.
mkdir -p "$hook_dir"
cp -f "$repo_session" "$hook_dir/claude_session_start_hook.py"
cp -f "$repo_stop" "$hook_dir/claude_stop_hook.py"
chmod 0755 "$hook_dir/claude_session_start_hook.py" "$hook_dir/claude_stop_hook.py"

mkdir -p "$settings_dir"
[[ -f "$settings_path" ]] || echo "{}" > "$settings_path"

# Strip any prior session/stop hook entries (stale absolute paths, wrong-home
# paths, legacy rc/ locations — matched by script basename), then install the
# canonical project-relative commands exactly once each.
tmp="$(mktemp)"
jq --arg sess "$sess_cmd" --arg stop "$stop_cmd" '
  def strip($base):
    map(.hooks |= map(select((.command // "") | endswith($base) | not)))
    | map(select((.hooks | length) > 0));
  .hooks //= {}
  | .hooks.SessionStart = ((.hooks.SessionStart // []) | strip("claude_session_start_hook.py"))
      + [{"hooks": [{"type": "command", "command": $sess}]}]
  | .hooks.Stop = ((.hooks.Stop // []) | strip("claude_stop_hook.py"))
      + [{"hooks": [{"type": "command", "command": $stop}]}]
' "$settings_path" > "$tmp"
mv "$tmp" "$settings_path"

echo "install_hook.sh: hooks installed (project-relative) → $hook_dir"
echo "settings: $settings_path"
