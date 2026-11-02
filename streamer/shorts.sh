#!/bin/bash
# Records a 30-second vertical update once a day and uploads it as a YouTube Short when credentials are set.
# "shorts.sh once" makes one right away. Files stay in $SHORTS_DIR, the last 14 days.
set -uo pipefail

SHORTS_DIR="${SHORTS_DIR:-/recordings/shorts}"
SHORT_HOUR_UTC="${SHORT_HOUR_UTC:-18}"
SHORT_DISPLAY=:98
MAX_UPLOAD_TRIES=3

log() { echo "[shorts] $(date -u +%H:%M:%S) $*"; }

record() {
  local file=$1 xvfb browser status track
  rm -f "/tmp/.X${SHORT_DISPLAY#:}-lock" "/tmp/.X11-unix/X${SHORT_DISPLAY#:}"
  Xvfb "$SHORT_DISPLAY" -screen 0 1080x1920x24 -nolisten tcp -ac >/dev/null 2>&1 &
  xvfb=$!
  sleep 2
  rm -rf /tmp/chromium-short
  local start_at=$(( $(date +%s%3N) + 10000 ))
  DISPLAY=$SHORT_DISPLAY chromium \
    --no-sandbox --no-first-run --noerrdialogs --disable-infobars --kiosk \
    --window-position=0,0 --window-size=1080,1920 --force-device-scale-factor=1 \
    --hide-scrollbars --disable-translate --disable-features=Translate,MediaRouter \
    --disable-dev-shm-usage --user-data-dir=/tmp/chromium-short \
    --app="${DASHBOARD_URL%/}/short.html?start=$start_at" >/dev/null 2>&1 &
  browser=$!
  sleep "$(awk -v at="$start_at" -v now="$(date +%s%3N)" 'BEGIN { printf "%.3f", (at - now) / 1000 }')"

  shopt -s nullglob
  local tracks=(/music/*.mp3 /music/*.m4a /music/*.ogg /music/*.flac /music/*.wav)
  local audio=(-f lavfi -i anullsrc=channel_layout=stereo:sample_rate=44100)
  if [ ${#tracks[@]} -gt 0 ]; then
    track=${tracks[RANDOM % ${#tracks[@]}]}
    audio=(-i "$track")
  fi
  DISPLAY="${SHORT_DISPLAY}.0" WIDTH=1080 HEIGHT=1920 AUDIO_BITRATE=160k RECORD_SECONDS=31 TRACK="${track:-}" \
    OUTPUT="$file.part.mp4" /usr/local/bin/anyps5-live-streamer encode
  status=$?
  kill "$browser" "$xvfb" 2>/dev/null
  wait "$browser" "$xvfb" 2>/dev/null
  rm -rf /tmp/chromium-short
  [ "$status" -eq 0 ] && mv "$file.part.mp4" "$file"
  return "$status"
}

can_upload() { [ -n "${YOUTUBE_CLIENT_ID:-}" ] && [ -n "${YOUTUBE_CLIENT_SECRET:-}" ] && [ -n "${YOUTUBE_REFRESH_TOKEN:-}" ]; }

upload() {
  local file=$1
  /usr/local/bin/anyps5-live-streamer upload-short "$file" || { log "Upload failed"; return 1; }
}

make_short() {
  local file=$1
  if [ ! -f "$file" ]; then
    log "Recording $file"
    record "$file" || { log "Recording failed"; return; }
    /usr/local/bin/anyps5-live-streamer short-meta > "$file.json" || log "Could not read the title"
    ls -1t "$SHORTS_DIR"/*.mp4 2>/dev/null | tail -n +15 | while read -r old; do rm -f "$old" "$old.json" "$old.uploaded" "$old.tries"; done
  fi
  can_upload && [ -f "$file.json" ] && [ ! -f "$file.uploaded" ] || return
  local tries
  tries=$(cat "$file.tries" 2>/dev/null || echo 0)
  [ "$tries" -lt "$MAX_UPLOAD_TRIES" ] || return
  echo $(( tries + 1 )) > "$file.tries"
  upload "$file" && touch "$file.uploaded"
}

mkdir -p "$SHORTS_DIR"
if [ "${1:-}" = once ]; then
  make_short "$SHORTS_DIR/$(date -u +%F).mp4"
  exit
fi
log "One Short a day from ${SHORT_HOUR_UTC}:00 UTC, upload $(can_upload && echo on || echo off)"
while true; do
  [ "$(date -u +%-H)" -ge "$SHORT_HOUR_UTC" ] && make_short "$SHORTS_DIR/$(date -u +%F).mp4"
  sleep 300
done
