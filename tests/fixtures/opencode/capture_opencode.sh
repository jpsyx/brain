#!/bin/sh
# Offline resolved-config output and argv capture; never starts an agent.
case " $* " in
  *" debug config --pure "*)
    if [ -n "${OPENCODE_CONFIG_CONTENT:-}" ]; then
      printf '%s\n' "$OPENCODE_CONFIG_CONTENT"
    else
      printf '{}\n'
    fi
    ;;
  *) printf '%s\0' "$@" ;;
esac
