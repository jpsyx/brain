#!/bin/sh
set -eu

log=${BRAIN_FAKE_CURL_LOG:?BRAIN_FAKE_CURL_LOG is required}
cat > "$log"
printf '{}\n'
