#!/bin/sh
set -eu

if [ "${SHORTS:-1}" = 1 ] && [ -z "${RECORD_SECONDS:-}" ]; then
  /usr/local/bin/shorts.sh &
fi

exec /usr/local/bin/anyps5-live-streamer
