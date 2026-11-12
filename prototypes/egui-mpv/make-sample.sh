#!/usr/bin/env bash
# 60s test pattern, two sine-tone audio tracks (eng 440Hz, fra 880Hz), one SRT track with a line every 5s.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p sample
# SRT lives in a temp dir: next to the mkv, mpv would auto-load it as a second, external subtitle track.
srt=$(mktemp --suffix=.srt)
trap 'rm -f "$srt"' EXIT
for i in $(seq 0 11); do
  s=$((i*5)); e=$((s+4))
  printf '%d\n00:%02d:%02d,000 --> 00:%02d:%02d,000\nSubtitle line %d at %ds\n\n' \
    $((i+1)) $((s/60)) $((s%60)) $((e/60)) $((e%60)) $((i+1)) $s
done > "$srt"
ffmpeg -y -loglevel error \
  -f lavfi -i testsrc=duration=60:size=1280x720:rate=30 \
  -f lavfi -i sine=frequency=440:duration=60 \
  -f lavfi -i sine=frequency=880:duration=60 \
  -i "$srt" \
  -map 0:v -map 1:a -map 2:a -map 3:s \
  -c:v libx264 -pix_fmt yuv420p -c:a aac -c:s srt \
  -metadata:s:a:0 language=eng -metadata:s:a:1 language=fra -metadata:s:s:0 language=eng \
  sample/sample.mkv
