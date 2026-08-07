#!/bin/sh
set -eu

if [ -n "${OPENCODE_TEST_LOG:-}" ]; then
    printf '%s|' "$PWD" >> "$OPENCODE_TEST_LOG"
    index=0
    for argument in "$@"; do
        if [ "$index" -ge 64 ]; then
            printf ' [truncated]' >> "$OPENCODE_TEST_LOG"
            break
        fi
        if [ "$index" -gt 0 ]; then
            printf ' ' >> "$OPENCODE_TEST_LOG"
        fi
        printf '%s' "$argument" | head -c 4096 >> "$OPENCODE_TEST_LOG"
        index=$((index + 1))
    done
    printf '\n' >> "$OPENCODE_TEST_LOG"
fi

case "$*" in
    "--version")
        printf '1.18.14\n'
        ;;
    "--help")
        printf '%s\n' '--agent --prompt --session'
        ;;
    "session list --help")
        printf '%s\n' '--format json'
        ;;
    "debug config --help")
        printf '%s\n' '--pure'
        ;;
    "debug config --pure"|"debug config")
        printf '{}\n'
        ;;
    "session list --format json")
        printf '[{"id":"session-1","directory":"%s","unknown":true},{"id":"session-1","directory":"%s"},{"id":"child","directory":"%s","parentID":"session-1"},{"id":"wrong-root","directory":"%s/other"}]\n' "$PWD" "$PWD" "$PWD" "$PWD"
        ;;
    *"--agent brain"*)
        if [ -n "${OPENCODE_TEST_LOG:-}" ]; then
            printf 'launch|%s\n' "$PWD" >> "$OPENCODE_TEST_LOG"
            index=0
            for argument in "$@"; do
                if [ "$index" -ge 64 ]; then
                    printf 'arg|truncated\n' >> "$OPENCODE_TEST_LOG"
                    break
                fi
                printf 'arg|%s|' "$index" >> "$OPENCODE_TEST_LOG"
                printf '%s' "$argument" | head -c 4096 >> "$OPENCODE_TEST_LOG"
                printf '\n' >> "$OPENCODE_TEST_LOG"
                index=$((index + 1))
            done
            for name in BRAIN_ACTOR_ID BRAIN_AGENT_KIND BRAIN_CHANNEL BRAIN_ROOT BRAIN_WORKSPACE BRAIN_WORKSPACE_ID OPENCODE_CONFIG_CONTENT; do
                if printenv "$name" >/dev/null 2>&1; then
                    printf 'env|%s\n' "$name" >> "$OPENCODE_TEST_LOG"
                fi
            done
            input_hex=$({ head -c 65536; cat >/dev/null; } | od -An -tx1 -v | tr -d ' \n')
            printf 'input|%s\n' "$input_hex" >> "$OPENCODE_TEST_LOG"
        else
            cat >/dev/null
        fi
        ;;
    *)
        printf 'unexpected fake OpenCode invocation: %s\n' "$*" >&2
        exit 64
        ;;
esac
