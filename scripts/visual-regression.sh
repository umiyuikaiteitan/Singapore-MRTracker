#!/usr/bin/env bash
#
# Render the example pages with a headless browser and compare them
# with the committed baselines.
#
# The Rust tests check the visual grammar and pin the SVG byte for
# byte. This script adds the pixel check that a stylesheet change can
# still slip past, for example a heading that stops standing out or a
# column that stops fitting on the page.
#
# Usage:
#   scripts/visual-regression.sh            compare with the baselines
#   scripts/visual-regression.sh --update   write new baselines
#
# Requirements:
#   - a Chromium or Chrome binary, named by $CHROME or found on PATH
#   - python3 with no extra packages
#
# Tolerance:
#   A run passes when the mean absolute difference per channel is at
#   most 2.0 of 255, and at most 1% of pixels differ by more than 32.
#   Font availability changes text rendering, so a baseline taken on
#   one machine will not match another machine exactly. Regenerate the
#   baselines on the machine you compare on.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
examples="$root/examples"
baselines="$root/examples/baseline"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

update=0
if [[ "${1:-}" == "--update" ]]; then
  update=1
fi

chrome="${CHROME:-}"
if [[ -z "$chrome" ]]; then
  for candidate in chromium chromium-browser google-chrome google-chrome-stable \
                   /opt/pw-browsers/chromium-*/chrome-linux/chrome; do
    if command -v "$candidate" >/dev/null 2>&1; then chrome="$candidate"; break; fi
    if [[ -x "$candidate" ]]; then chrome="$candidate"; break; fi
  done
fi
if [[ -z "$chrome" ]]; then
  echo "no Chromium or Chrome binary found; set CHROME=/path/to/chrome" >&2
  exit 1
fi

echo "Regenerating the example pages ..."
cargo test -q -p mrt-publication-html --test visual_tests

shoot() { # name width height
  "$chrome" --headless --disable-gpu --no-sandbox --hide-scrollbars \
    --force-device-scale-factor=1 --window-size="$2,$3" \
    --screenshot="$work/$1.png" "file://$examples/$1.html" >/dev/null 2>&1
}

echo "Rendering ..."
shoot timetable-woodlands 1280 2200
shoot diagram-tel 1500 1200

if [[ "$update" == "1" ]]; then
  mkdir -p "$baselines"
  cp "$work"/*.png "$baselines/"
  echo "Wrote the baselines to $baselines."
  exit 0
fi

status=0
for name in timetable-woodlands diagram-tel; do
  baseline="$baselines/$name.png"
  if [[ ! -f "$baseline" ]]; then
    echo "no baseline for $name; run with --update" >&2
    status=1
    continue
  fi
  if python3 "$root/scripts/compare-png.py" "$baseline" "$work/$name.png" "$name"; then
    :
  else
    status=1
  fi
done

exit "$status"
