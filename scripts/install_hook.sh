#!/usr/bin/env bash
# Idempotently install brain's Claude Code hooks into ~/brain/.claude/settings.json.
#
# Deploys the two hook scripts into ~/brain/.claude/brain-hooks/ (so they travel
# with the synced brain dir) and registers them with HOME-relative (`~/…`)
# commands, so the synced settings.json resolves on EVERY machine regardless of
# home dir (/Users/pablo vs /Users/juanpablosarmiento). An absolute path would
# bake one machine's home into the synced config and break everywhere else.
#
# This writes exactly the same entries as `brain receiver setup`
# (install_receiver_hooks in src/main.rs → hook_command), so the two installers
# are idempotent with each other. Both hooks:
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

hook_dir="${HOME}/brain/.claude/brain-hooks"
settings_dir="${HOME}/brain/.claude"
settings_path="${settings_dir}/settings.json"

# HOME-relative commands — identical to install_receiver_hooks' hook_command output.
sess_cmd="python3 ~/brain/.claude/brain-hooks/claude_session_start_hook.py"
stop_cmd="python3 ~/brain/.claude/brain-hooks/claude_stop_hook.py"

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
# canonical HOME-relative commands exactly once each.
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

echo "install_hook.sh: hooks installed (HOME-relative) → $hook_dir"
echo "settings: $settings_path"
