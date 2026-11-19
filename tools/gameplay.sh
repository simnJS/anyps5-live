#!/bin/bash
# Prepares a gameplay clip for the streams: 1080p at 30 fps, H.264, no sound, playable from its first second.
# It writes gameplay/<name>.mp4 and gameplay/gameplay.json, the only clip there; the pages play it on the clock.
#
#   tools/gameplay.sh <video> [options]
#     -s, --start <time>      where the clip starts in the video (90, 1:30), from the start by default
#     -m, --minutes <n>       how long the clip lasts, the rest of the video by default
#     -t, --title <text>      default: "GTA V on AnyKyty"
#     -c, --caption <text>    default: "Fidelity 4K with ray tracing · RTX 4080 SUPER · recorded <the video's date>"
#     -e, --every <minutes>   how often it plays, default 30 (on the clock: :00 and :30)
#
# Then copy the folder to the server; the pages pick the new clip up within a minute, with no restart:
#   rsync -avP --delete gameplay/ <user>@<server>:anyps5-live/gameplay/
set -euo pipefail

usage() { sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

[ $# -ge 1 ] || usage 1
case "$1" in -h|--help) usage ;; esac
source_video=$1
shift
[ -f "$source_video" ] || { echo "No such video: $source_video" >&2; exit 1; }

start=0 minutes="" title="GTA V on AnyKyty" caption="" every=30
while [ $# -gt 0 ]; do
  case "$1" in
    -s|--start) start=$2; shift 2 ;;
    -m|--minutes) minutes=$2; shift 2 ;;
    -t|--title) title=$2; shift 2 ;;
    -c|--caption) caption=$2; shift 2 ;;
    -e|--every) every=$2; shift 2 ;;
    -h|--help) usage ;;
    *) echo "Unknown option: $1" >&2; usage 1 ;;
  esac
done

recorded=$(date -r "$source_video" +"%b %-d")
[ -n "$caption" ] || caption="Fidelity 4K with ray tracing · RTX 4080 SUPER · recorded $recorded"

folder="$(cd "$(dirname "$0")/.." && pwd)/gameplay"
mkdir -p "$folder"
name="gta5-$(date -r "$source_video" +%Y%m%d-%H%M).mp4"
length=()
[ -n "$minutes" ] && length=(-t "$(awk -v m="$minutes" 'BEGIN { print m * 60 }')")

echo "Encoding $source_video into gameplay/$name"
ffmpeg -hide_banner -loglevel error -stats -y -ss "$start" -i "$source_video" "${length[@]}" \
  -vf "scale=1920:1080:force_original_aspect_ratio=decrease:flags=lanczos,pad=1920:1080:-1:-1:color=black,fps=30,format=yuv420p" \
  -c:v libx264 -preset slow -crf 20 -maxrate 8M -bufsize 16M -profile:v high -level 4.1 -g 60 \
  -an -movflags +faststart "$folder/$name.part.mp4"
mv "$folder/$name.part.mp4" "$folder/$name"

seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$folder/$name")
python3 - "$folder/gameplay.json" "$name" "$title" "$caption" "$every" "$seconds" <<'PY'
import json, sys
path, name, title, caption, every, seconds = sys.argv[1:]
with open(path, "w", encoding="utf-8") as out:
    every = float(every)
    json.dump({"file": name, "title": title, "caption": caption, "everyMinutes": int(every) if every.is_integer() else every,
               "seconds": round(float(seconds), 1)},
              out, ensure_ascii=False, indent=2)
    out.write("\n")
PY
# One clip at a time: the older ones would only weigh on the copy to the server.
find "$folder" -maxdepth 1 -name '*.mp4' ! -name "$name" -delete

size=$(du -h "$folder/$name" | cut -f1)
printf 'Ready: gameplay/%s, %s, %.0f s, every %s minutes\n' "$name" "$size" "$seconds" "$every"
echo "Copy it to the server: rsync -avP --delete gameplay/ <user>@<server>:anyps5-live/gameplay/"
