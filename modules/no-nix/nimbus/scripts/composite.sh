#!/usr/bin/env bash
# Composite `nimbus --snapshot` frames over a wallpaper the way niri would:
# blurred + saturated wallpaper inside the glass shapes, sharp elsewhere.
#
#   scripts/composite.sh <snapshot-dir> <wallpaper> <frame-name>...
#
# Needs ImageMagick (`nix-shell -p imagemagick`). Writes <frame-name>.png.
set -euo pipefail
dir=$1 wall=$2
shift 2

pams=("$dir"/*.pam)
first=${pams[0]}
read -r W H < <(magick identify -format '%w %h\n' "$first")

if [[ ! -f $dir/_wall.png ]]; then
  magick "$wall" -resize 3200x2000^ -gravity north -extent 3200x2000 \
    -crop "${W}x${H}+0+0" +repage "$dir/_wall.png"
  # niri: blur passes 4, saturation 1.35 → a wide gaussian is close enough.
  magick "$dir/_wall.png" -blur 0x16 -modulate 100,135 "$dir/_blur.png"
fi

for name in "$@"; do
  draw=""
  while read -r x y w h; do
    r=$(awk "BEGIN{print $h/2}")
    draw+="roundrectangle $x,$y $(awk "BEGIN{print $x+$w}"),$(awk "BEGIN{print $y+$h}") $r,$r "
  done < "$dir/$name.glass"
  magick -size "${W}x${H}" xc:black -fill white ${draw:+-draw "$draw"} "$dir/_mask.png"
  magick "$dir/_wall.png" "$dir/_blur.png" "$dir/_mask.png" -composite \
    "$dir/$name.pam" -composite "$dir/$name.png"
done
