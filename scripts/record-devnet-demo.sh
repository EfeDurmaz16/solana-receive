#!/usr/bin/env bash
# Real browser screen-capture of Devnet Explorer txs + local demo UI.
# No generated animation — records Chrome navigating live pages.
set -euo pipefail
export DISPLAY=:1
export PATH="${HOME}/.local/bin:${PATH}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ART_DIR="/opt/cursor/artifacts"
mkdir -p "$ART_DIR"
RAW="$ART_DIR/solana-receive-devnet-raw.mkv"
OUT="$ART_DIR/solana-receive-devnet-demo.mp4"
JSON="$ROOT/demos/receive/last-run.json"

python3 - <<'PY' > /tmp/demo-urls.txt
import json
from pathlib import Path
d = json.loads(Path("/workspace/demos/receive/last-run.json").read_text())
print("TITLE|https://explorer.solana.com/address/%s?cluster=devnet" % d["programId"])
print("BEFORE|" + d["explorers"]["before"])
print("CREDITED|" + d["explorers"]["credited"])
print("HELD|" + d["explorers"]["held"])
print("CLAIM|" + d["explorers"]["claim"])
print("EXPIRY|" + d["explorers"]["expiry"])
print("UI|http://127.0.0.1:8765/")
PY

# Kill prior chrome/ffmpeg
pkill -f 'google-chrome' 2>/dev/null || true
pkill -f 'chromium' 2>/dev/null || true
pkill -f 'ffmpeg.*solana-receive-devnet' 2>/dev/null || true
sleep 1

# Solid dark backdrop
xsetroot -solid '#0b0f14' 2>/dev/null || true

PROFILE="/tmp/chrome-demo-profile"
rm -rf "$PROFILE"
mkdir -p "$PROFILE"

# Start Chrome maximized-ish
google-chrome \
  --user-data-dir="$PROFILE" \
  --no-first-run \
  --disable-infobars \
  --disable-session-crashed-bubble \
  --window-size=1440,900 \
  --window-position=40,40 \
  --force-device-scale-factor=1 \
  "about:blank" >/tmp/chrome-demo.log 2>&1 &
CHROME_PID=$!
sleep 3

# Focus chrome window
WID=$(xdotool search --onlyvisible --class 'Google-chrome|Chromium|chrome' 2>/dev/null | head -n1 || true)
if [[ -z "$WID" ]]; then
  WID=$(xdotool search --onlyvisible --name 'Chrome\|Chromium\|about' 2>/dev/null | head -n1 || true)
fi
echo "chrome_wid=$WID pid=$CHROME_PID"
if [[ -n "$WID" ]]; then
  xdotool windowactivate --sync "$WID" || true
  xdotool windowmove "$WID" 40 40 || true
  xdotool windowsize "$WID" 1440 900 || true
fi

# Start screen recording (full display crop to 1440x900 around chrome)
rm -f "$RAW" "$OUT"
ffmpeg -y -nostdin \
  -f x11grab -video_size 1440x900 -framerate 30 -i :1.0+40,40 \
  -c:v libx264 -preset veryfast -pix_fmt yuv420p -crf 20 \
  "$RAW" >/tmp/ffmpeg-demo.log 2>&1 &
FF_PID=$!
sleep 1
echo "recording pid=$FF_PID"

navigate() {
  local label="$1" url="$2" hold="${3:-7}"
  echo "== $label =="
  if [[ -n "$WID" ]]; then
    xdotool windowactivate --sync "$WID" || true
  fi
  # Focus omnibox and navigate
  xdotool key --clearmodifiers ctrl+l
  sleep 0.35
  xdotool type --delay 8 --clearmodifiers "$url"
  sleep 0.2
  xdotool key --clearmodifiers Return
  # Wait for page load + dwell so balances/tx details are visible
  sleep "$hold"
  # Gentle scroll to show money-flow / token balances
  xdotool key --clearmodifiers Next
  sleep 1.2
  xdotool key --clearmodifiers Next
  sleep 1.0
  xdotool key --clearmodifiers Prior
  sleep 0.8
}

# Scene timing aimed at ~50–60s total
while IFS='|' read -r label url; do
  case "$label" in
    TITLE) navigate "program" "$url" 6 ;;
    BEFORE) navigate "before SPL" "$url" 8 ;;
    CREDITED) navigate "credited" "$url" 7 ;;
    HELD) navigate "held" "$url" 8 ;;
    CLAIM) navigate "claim" "$url" 7 ;;
    EXPIRY) navigate "expiry" "$url" 7 ;;
    UI) navigate "demo UI" "$url" 8 ;;
  esac
done < /tmp/demo-urls.txt

sleep 1
kill -INT "$FF_PID" 2>/dev/null || true
wait "$FF_PID" 2>/dev/null || true

# Transcode to compact mp4
ffmpeg -y -nostdin -i "$RAW" \
  -c:v libx264 -preset medium -pix_fmt yuv420p -crf 22 -movflags +faststart \
  "$OUT" >/tmp/ffmpeg-transcode.log 2>&1

# Stop chrome
kill "$CHROME_PID" 2>/dev/null || true

ls -lh "$OUT" "$RAW"
ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT"
echo "WROTE $OUT"
