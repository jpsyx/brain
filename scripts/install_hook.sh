#!/usr/bin/env bash
# Idempotently install the brain SessionStart hook into
# ~/brain/.claude/settings.json.
#
# The hook records which Claude session the merged brain shell's brain panel is
# driving, so the panel can resume it later (lock + recency). After the tasks↔
# brain merge there is exactly ONE hook: this one, keyed on the BRAIN_* env
# vars. It also strips any stale entries left behind by the pre-merge world:
#   - the standalone `tasks` SessionStart hook (…/rc/tasks/scripts/…), and
#   - the legacy `claude_stop_hook.py` Stop hook from the old queueing system.
#
# Safe to re-run. Bails (non-zero) if jq is missing — we lean on jq to do the
# merge so we don't accidentally corrupt the user's settings.

set -euo pipefail

script_dir="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
hook_path="${script_dir}/claude_session_start_hook.py"
stop_hook_path="${script_dir}/claude_stop_hook.py"
settings_dir="${HOME}/brain/.claude"
settings_path="${settings_dir}/settings.json"

if ! command -v jq >/dev/null 2>&1; then
  echo "install_hook.sh: jq is required (brew install jq)" >&2
  exit 1
fi

if [[ ! -f "$hook_path" ]]; then
  echo "install_hook.sh: hook script missing at $hook_path" >&2
  exit 1
fi
if [[ ! -f "$stop_hook_path" ]]; then
  echo "install_hook.sh: stop hook script missing at $stop_hook_path" >&2
  exit 1
fi

mkdir -p "$settings_dir"

if [[ ! -f "$settings_path" ]]; then
  echo "{}" > "$settings_path"
fi

# Compose:
#  1. Drop the legacy tasks Stop-hook entry (claude_stop_hook.py).
#  2. Drop the legacy standalone tasks SessionStart hook (…/rc/tasks/scripts/…).
#  3. Ensure .hooks.SessionStart contains our brain entry exactly once.
tmp="$(mktemp)"
jq --arg cmd "$hook_path" --arg stopcmd "$stop_hook_path" '
  .hooks //= {} |
  # 1. Strip the legacy tasks Stop hook, if present.
  (if (.hooks | has("Stop")) then
     .hooks.Stop |= map(
       .hooks |= map(select((.command // "") | endswith("claude_stop_hook.py") | not))
     ) | .hooks.Stop |= map(select((.hooks | length) > 0))
   else . end) |
  (if (.hooks.Stop? | (. == null or length == 0)) then .hooks |= del(.Stop) else . end) |
  # 2. Strip the standalone tasks SessionStart hook, if present.
  .hooks.SessionStart //= [] |
  .hooks.SessionStart |= map(
    .hooks |= map(select(
      (.command // "") | endswith("rc/tasks/scripts/claude_session_start_hook.py") | not
    ))
  ) | .hooks.SessionStart |= map(select((.hooks | length) > 0)) |
  # 3. Ensure the brain SessionStart hook is installed exactly once.
  if (.hooks.SessionStart | map(.hooks // [] | map(.command) | flatten | index($cmd)) | any) then
    .
  else
    .hooks.SessionStart += [{
      "hooks": [{ "type": "command", "command": $cmd }]
    }]
  end |
  # 4. Ensure the brain Stop hook is installed exactly once.
  .hooks.Stop //= [] |
  if (.hooks.Stop | map(.hooks // [] | map(.command) | flatten | index($stopcmd)) | any) then
    .
  else
    .hooks.Stop += [{ "hooks": [{ "type": "command", "command": $stopcmd }] }]
  end
' "$settings_path" > "$tmp"

mv "$tmp" "$settings_path"

echo "install_hook.sh: hook installed → $hook_path"
echo "settings: $settings_path"
