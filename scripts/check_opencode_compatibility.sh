#!/bin/sh
set -eu

if [ "$#" -eq 0 ]; then
    open_code_command=${OPENCODE_CMD:-opencode}
else
    open_code_command=$*
fi

probe_root=$(mktemp -d "${TMPDIR:-/tmp}/brain-opencode-compatibility.XXXXXX")
trap 'rm -rf "$probe_root"' EXIT HUP INT TERM
probe_home="$probe_root/home"
probe_workspace="$probe_root/workspace"
mkdir -p "$probe_home" "$probe_workspace/.opencode/plugins" "$probe_workspace/.claude/brain-hooks"

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cp "$script_dir/opencode_brain_plugin.js" "$probe_workspace/.opencode/plugins/brain.js"
cp "$script_dir/agent_session_start_hook.py" "$probe_workspace/.claude/brain-hooks/agent_session_start_hook.py"
cp "$script_dir/agent_turn_complete_hook.py" "$probe_workspace/.claude/brain-hooks/agent_turn_complete_hook.py"

config='{"default_agent":"brain","agent":{"brain":{"mode":"primary","prompt":"Brain compatibility probe","permission":{"skill":{"*":"deny","probe":"allow"}}}},"mcp":{"brain_ws_probe":{"type":"local","command":["/usr/bin/true"],"enabled":false}},"skills":{"paths":[]}}'

run_probe() {
    (
        cd "$probe_workspace"
        HOME="$probe_home" \
        XDG_CONFIG_HOME="$probe_home/config" \
        XDG_CACHE_HOME="$probe_home/cache" \
        XDG_DATA_HOME="$probe_home/data" \
        XDG_STATE_HOME="$probe_home/state" \
        BRAIN_ROOT="$probe_workspace" \
        BRAIN_AGENT_KIND=opencode \
        OPENCODE_CONFIG_CONTENT="$config" \
        /bin/sh -c "$open_code_command $1"
    )
}

version=$(run_probe --version 2>&1 | awk 'NR == 1 {print $1}')
help=$(run_probe --help 2>&1)
for option in --agent --prompt --session; do
    printf '%s\n' "$help" | grep -F -- "$option" >/dev/null
done

session_help=$(run_probe 'session list --help' 2>&1)
printf '%s\n' "$session_help" | grep -F -- '--format' >/dev/null
printf '%s\n' "$session_help" | grep -i -- 'json' >/dev/null

session_json=$(run_probe 'session list --format json')
printf '%s' "$session_json" | python3 -c 'import json,sys; raw=sys.stdin.read(); value=[] if not raw.strip() else json.loads(raw); assert isinstance(value, list)'

config_help=$(run_probe 'debug config --help' 2>&1)
printf '%s\n' "$config_help" | grep -F -- '--pure' >/dev/null
for arguments in 'debug config --pure' 'debug config'; do
    resolved=$(run_probe "$arguments")
    printf '%s' "$resolved" | python3 -c 'import json,sys; value=json.load(sys.stdin); assert isinstance(value, dict)'
done

printf 'OpenCode %s is compatible with Brain\n' "$version"
