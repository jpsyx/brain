#!/bin/sh
set -eu

# A stand-in pi for Brain's tests: it answers the three read-only questions the
# compatibility probe asks and records a launch, so no test depends on the
# developer's own pi installation, configuration, or provider credentials.

if [ -n "${PI_TEST_LOG:-}" ]; then
    printf '%s|' "$PWD" >> "$PI_TEST_LOG"
    index=0
    for argument in "$@"; do
        if [ "$index" -ge 64 ]; then
            printf ' [truncated]' >> "$PI_TEST_LOG"
            break
        fi
        if [ "$index" -gt 0 ]; then
            printf ' ' >> "$PI_TEST_LOG"
        fi
        printf '%s' "$argument" | head -c 4096 >> "$PI_TEST_LOG"
        index=$((index + 1))
    done
    printf '\n' >> "$PI_TEST_LOG"
fi

case "$*" in
    "--version")
        printf '0.85.1\n'
        ;;
    "--help")
        printf '%s\n' '--session-id --append-system-prompt --extension --skill --no-skills --no-approve'
        ;;
    "--list-models")
        printf 'provider      model          context  max-out  thinking  images\n'
        printf 'fake          fake-model     1M       16.4K    yes       no\n'
        ;;
    *"--session-id"*)
        if [ -n "${PI_TEST_LOG:-}" ]; then
            printf 'launch|%s\n' "$PWD" >> "$PI_TEST_LOG"
            index=0
            for argument in "$@"; do
                if [ "$index" -ge 64 ]; then
                    printf 'arg|truncated\n' >> "$PI_TEST_LOG"
                    break
                fi
                printf 'arg|%s|' "$index" >> "$PI_TEST_LOG"
                printf '%s' "$argument" | head -c 4096 >> "$PI_TEST_LOG"
                printf '\n' >> "$PI_TEST_LOG"
                index=$((index + 1))
            done
            for name in BRAIN_ACTOR_ID BRAIN_AGENT_KIND BRAIN_CHANNEL BRAIN_ROOT BRAIN_WORKSPACE BRAIN_WORKSPACE_ID; do
                if printenv "$name" >/dev/null 2>&1; then
                    printf 'env|%s\n' "$name" >> "$PI_TEST_LOG"
                fi
            done
            input_hex=$({ head -c 65536; cat >/dev/null; } | od -An -tx1 -v | tr -d ' \n')
            printf 'input|%s\n' "$input_hex" >> "$PI_TEST_LOG"
        else
            cat >/dev/null
        fi
        ;;
    *)
        printf 'unexpected fake pi invocation: %s\n' "$*" >&2
        exit 64
        ;;
esac
