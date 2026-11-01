#!/usr/bin/env bash
# Render a sheet of looks (or designs) over a wallpaper, one row each.
#   scripts/looks-sheet.sh <out.png> <wallpaper> <name>...
# Names are NIMBUS_LOOK values; set SHEET_VAR=NIMBUS_DESIGN for designs.
# Needs ImageMagick and the vulkan loader on LD_LIBRARY_PATH.
set -euo pipefail
out=$1 wall=$2
shift 2
here=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d "${out%.png}.XXXX")
rows=()
for L in "$@"; do
  d=$work/$L
  env "${SHEET_VAR:-NIMBUS_LOOK}=$L" NIMBUS_SNAPSHOT_STILLS=1 "$here/target/release/nimbus" --snapshot "$d" >/dev/null 2>&1
  "$here/scripts/composite.sh" "$d" "$wall" idle hover_clock_rest hover_ws_rest
  if [[ ${SHEET_FULL:-} ]]; then
    # Full-width rows: idle, workspace hover, clock hover.
    magick -size 3199x60 xc:'#111111' -fill white -pointsize 34 -gravity west -annotate +20+0 "$L" "$d/label.png"
    magick "$d/label.png" "$d/idle.png" "$d/hover_ws_rest.png" "$d/hover_clock_rest.png" -append "$d/row.png"
  else
    magick "$d/hover_ws_rest.png" -crop 700x81+0+0 +repage "$d/l.png"
    magick "$d/idle.png" -crop 900x81+1150+0 +repage "$d/m.png"
    magick "$d/hover_clock_rest.png" -crop 1150x81+2049+0 +repage "$d/r.png"
    magick -size 230x81 xc:'#111111' -fill white -pointsize 34 -gravity center -annotate +0+0 "$L" "$d/label.png"
    magick "$d/label.png" "$d/l.png" "$d/m.png" "$d/r.png" +append "$d/row.png"
  fi
  rows+=("$d/row.png")
done
magick "${rows[@]}" -append "$out"
rm -rf "$work"
