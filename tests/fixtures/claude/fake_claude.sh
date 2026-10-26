#!/bin/sh
case " $* " in
  *" --version "*) printf '%s\n' '2.1.308 (Claude Code)' ;;
  *" --help "*) printf '%s\n' '--append-system-prompt --append-system-prompt-file --system-prompt-snapshot <on|off>' ;;
  *) printf '%s\0' "$@" ;;
esac
