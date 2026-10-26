#!/bin/sh
# Offline instruction introspection and argv capture; never starts an agent.
case " $* " in
  *" mcp list --json "*) printf '%s\n' '[]' ;;
  *" debug prompt-input "*)
    printf '%s\n' '[{"type":"message","role":"developer","content":[{"type":"input_text","text":"Harness fixture"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["permissions.instructions"],"content_item_metadata":[{"provenance":{"type":"harness"}}]}}]'
    ;;
  *) printf '%s\0' "$@" ;;
esac
