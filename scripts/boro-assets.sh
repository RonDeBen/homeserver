#!/usr/bin/env bash
# Boro asset processing pipeline: turns the full-res source masters in
# assets/boro-src/ (gitignored) into small, transparent, web-ready WebP under
# crates/gateway/assets/boro/ (committed, vendored into the binary).
#
# Re-runnable: safe to run again whenever new/regenerated art lands in boro-src.
# Requires: ImageMagick 7 (`magick`).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/assets/boro-src"
OUT="$ROOT/crates/gateway/assets/boro"
Q="-define webp:method=4 -quality 82 -strip"

# Clean rebuild so removed/renamed source masters never leave stale outputs.
rm -rf "$OUT"; mkdir -p "$OUT"/{materials,labels,repairs,frays,sashiko,badges,smeech,masks}

# Flood-fill transparency in from all 4 corners — robust for the baked solid-gray
# AND checkerboard backgrounds (both connect to the corners; the fabric doesn't).
key() { # in out [fuzz%]
  magick "$1" -alpha set -fuzz "${3:-14}%" -fill none \
    -draw "alpha 0,0 floodfill" -draw "alpha %[fx:w-1],0 floodfill" \
    -draw "alpha 0,%[fx:h-1] floodfill" -draw "alpha %[fx:w-1],%[fx:h-1] floodfill" \
    -trim +repage "$2"
}
# Kill any leftover magenta/pink matte halo on already-transparent cutouts.
defringe() { magick "$1" -fuzz 20% -transparent magenta -trim +repage "$2"; }

echo "== materials (tiles; no key) =="
for m in linen charcoal faded-blue indigo olive rust; do
  magick "$SRC/materials/$m.png" -resize 640x640 $Q "$OUT/materials/$m.webp"
done

echo "== labels (per material; new masters already have alpha, so key is a no-op) =="
for m in linen charcoal faded-blue indigo olive rust; do
  n=1
  for f in "$SRC"/labels/label-$m-*.png; do
    [ -e "$f" ] || continue
    key "$f" /tmp/_k.png 13
    magick /tmp/_k.png -resize 520x $Q "$OUT/labels/label-$m-$(printf %02d $n).webp"; n=$((n+1))
  done
done

echo "== repairs (indigo loose cutouts, defringe) =="
n=1; for f in "$SRC"/repairs/repair-indigo-*.png; do
  defringe "$f" /tmp/_d.png
  magick /tmp/_d.png -resize 340x $Q "$OUT/repairs/repair-indigo-$(printf %02d $n).webp"; n=$((n+1))
done
echo "== repairs (denim, keyed) =="
n=1; for f in "$SRC"/repairs-src/repair-denim-*.png; do
  key "$f" /tmp/_k.png 14
  magick /tmp/_k.png -resize 340x $Q "$OUT/repairs/repair-denim-$(printf %02d $n).webp"; n=$((n+1))
done
echo "== repairs (rust from Tiny Badge, keyed) =="
# The rust Tiny Badge is really a small rust repair patch (has its own stitching).
key "$SRC/badges/badge-01.png" /tmp/_k.png 15; magick /tmp/_k.png -resize 300x $Q "$OUT/repairs/repair-rust-01.webp"

echo "== frays: 3 distress variants per material (real weave in 3 real tear masks) =="
# Three tear shapes with different distress so edges vary per card/colour (brief §1):
#   m1,m2 = the two heavily-frayed real denim cutouts (loose threads, more distressed)
#   m3    = a Frayed-Edge master, keyed (tidier, gentler fray)
FW=1024; FH=320
key "$SRC/frays-src/fray-denim-src-01.png" /tmp/_fk.png 16
magick /tmp/_fk.png -alpha extract -resize ${FW}x${FH}! /tmp/_m3.png
magick "$SRC/frays/fray-denim-01.png" -alpha extract -resize ${FW}x${FH}! /tmp/_m1.png
magick "$SRC/frays/fray-denim-02.png" -alpha extract -resize ${FW}x${FH}! /tmp/_m2.png
vi=1
for mask in /tmp/_m1.png /tmp/_m2.png /tmp/_m3.png; do
  for m in linen charcoal faded-blue indigo olive rust; do
    magick "$SRC/materials/$m.png" -resize "${FW}x${FH}^" -gravity center -extent "${FW}x${FH}" /tmp/_strip.png
    magick /tmp/_strip.png "$mask" -alpha off -compose CopyOpacity -composite -trim +repage $Q "$OUT/frays/fray-$m-$(printf %02d $vi).webp"
  done
  vi=$((vi+1))
done

echo "== sashiko (cream stitch runs, keyed) =="
n=1; for f in "$SRC"/sashiko/sashiko-*.png; do
  key "$f" /tmp/_k.png 18
  magick /tmp/_k.png -resize 420x $Q "$OUT/sashiko/sashiko-$(printf %02d $n).webp"; n=$((n+1))
done

echo "== badges (keyed) =="
n=1; for f in "$SRC"/badges/badge-*.png; do
  key "$f" /tmp/_k.png 15
  magick /tmp/_k.png -resize 150x $Q "$OUT/badges/badge-$(printf %02d $n).webp"; n=$((n+1))
done

echo "== repairs per material (real weave in a clean frayed-square alpha + stitch) =="
# Light fabrics can't be keyed off a light bg, so build the missing-material patches by
# pouring the real canvas weave into a clean boro-square alpha (from a good indigo cutout)
# and laying a real cream sashiko run across it for the hand-stitched read. Runs after
# sashiko so the stitch webp exists.
RMASK=/tmp/_rmask.png
magick "$SRC/repairs/repair-indigo-05.png" -alpha extract -resize 600x600 -morphology Erode Disk:1.5 "$RMASK"
for m in linen olive charcoal faded-blue; do
  magick "$SRC/materials/$m.png" -resize 600x600^ -gravity center -extent 600x600 /tmp/_p.png
  magick /tmp/_p.png "$RMASK" -alpha off -compose CopyOpacity -composite /tmp/_patch.png
  magick /tmp/_patch.png \( "$OUT/sashiko/sashiko-02.webp" -resize 470x -rotate -4 \) -gravity center -compose over -composite /tmp/_patch2.png
  magick /tmp/_patch2.png "$RMASK" -alpha off -compose CopyOpacity -composite -trim +repage -resize 320x $Q "$OUT/repairs/repair-$m-01.webp"
done

echo "== smeech (curated poses from the user's in-place extractions) =="
# The extracted pieces keep their original 1536x1024 canvas position, so a pair
# (cat + its prop) recomposes exactly by flattening the two full canvases; occluded
# regions are transparent in both, so stacking order doesn't matter. UUID->pose map
# is a manual identification of assets/boro-src/smeech/smeech_extracted_assets/.
SMX="$SRC/smeech/smeech_extracted_assets"
if [ -d "$SMX" ]; then
  declare -A SF=( [01]=04ef2110-7cb7-4e0f-b93a-4bbd06c2328e [02]=08d6ec4c-5bb3-4d9d-b8c2-363de497dda2 \
    [03]=0a3480a9-4f1f-416f-95af-282d2ca1ee3b [04]=0d347f90-e7d0-4685-9064-e19a2524d314 \
    [05]=142e69a7-b8b9-438f-b2ce-a06ef27a49b8 [07]=2a275992-0141-4e85-b6fb-966bca5a5a11 \
    [10]=4d9db66f-505e-48c2-a976-b0eb2a4c38e4 [11]=53699f6b-20e8-4504-8b9b-56f6b24ef16d \
    [12]=54586b64-98ca-4010-9f7d-fecbe9bca489 [13]=5a67642e-f815-4b96-95b6-1605e5c5588b \
    [14]=5e9897ce-b4f7-46a6-a709-f7e8386066d1 [18]=71bd85ba-e6c7-4db5-8d5d-c181b73ffe34 \
    [19]=7bab0c21-3049-495a-8fe0-b09bc280d010 [24]=a9231e2f-14e1-4eb1-a1ec-d7d7d010efd8 \
    [25]=afd62d90-0889-4489-a1b5-a55b45113ce5 [26]=b2479547-3ab2-4c00-a90f-e76ed2b21340 \
    [27]=b8fcf663-318a-4b30-b619-b20b9443b0eb [28]=c290fa8b-bfb9-4e5a-9aa5-1e044fe7bd07 \
    [30]=cc6aa26c-d6ed-47c2-a07e-dc81f63bd3c0 [31]=cd7cc6fa-3abb-4467-88cb-895c4ae427f1 \
    [33]=d6396272-1c4f-48df-b9bd-150ce5a8276b [34]=dc0e9b2d-12cb-4204-bbed-74e7fb6b856a \
    [37]=ffd62e6b-8c2b-4519-8cbc-892d9dea2e86 )
  sp() { echo "$SMX/${SF[$1]}.png"; }
  spair() { magick "$(sp $1)" "$(sp $2)" -background none -layers flatten -trim +repage -resize 460x460 $Q "$OUT/smeech/$3.webp"; }
  sone()  { magick "$(sp $1)" -trim +repage -resize 460x460 $Q "$OUT/smeech/$2.webp"; }
  # rest
  sone 31 smeech-rest-curled;      sone 33 smeech-rest-curled-lg;  sone 12 smeech-rest-sidesleep
  spair 04 05 smeech-rest-blanket
  # watch (incl. edge interactions)
  sone 25 smeech-watch-sit;        sone 18 smeech-watch-back;      sone 14 smeech-watch-loaf
  spair 01 13 smeech-watch-peek-ledge; spair 26 27 smeech-watch-peek-pocket; spair 11 24 smeech-watch-drape
  # active
  sone 28 smeech-active-walk;      sone 30 smeech-active-prowl;    sone 07 smeech-active-stretch
  # play / success + attention
  sone 03 smeech-play-pounce;      sone 37 smeech-play-back;       sone 34 smeech-attention-paw
  # extras (props / motifs)
  sone 19 smeech-extra-tail;       sone 02 smeech-extra-yarn;      sone 10 smeech-extra-pawbadge
else
  echo "  (skip: $SMX not present)"
fi

echo "== torn-edge masks (for the jagged-card silhouette prototype) =="
# White jagged rectangle on transparent → used as a CSS mask on a card's fabric
# background layer, so the card reads as a rough-cut cloth scrap (content unaffected).
# `-spread` randomly displaces edge pixels; different seeds → different tears.
for i in 1 2 3; do
  magick -size 840x560 xc:none -seed $((i*97)) \
    -fill white -draw "roundrectangle 24,24 816,536 14,14" \
    -channel A -spread 12 -blur 0x1.0 -threshold 45% +channel \
    "$OUT/masks/torn-$(printf %02d $i).png"
done

echo "== DONE =="
du -sh "$OUT"
find "$OUT" -name '*.webp' | wc -l | xargs echo "webp count:"
