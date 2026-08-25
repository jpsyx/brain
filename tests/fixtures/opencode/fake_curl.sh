#!/bin/sh
set -eu

log=${BRAIN_FAKE_CURL_LOG:?BRAIN_FAKE_CURL_LOG is required}
if [ -n "${BRAIN_FAKE_CURL_COUNT_LOG:-}" ]; then
  printf 'invocation\n' >> "$BRAIN_FAKE_CURL_COUNT_LOG"
fi
cat > "$log"
printf '{}\n'
