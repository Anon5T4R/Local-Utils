#!/usr/bin/env bash
# Regenerates the committed icon tree (assets/app-icon/hicolor) from the PNG
# master at icons/icon-1024.png.
#
# Needs: ImageMagick (`magick`). The outputs are committed, so builds and
# packaging never need these tools.
#
#   packaging/icons.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/icons/icon-1024.png"
DIR="$ROOT/assets/app-icon/hicolor"
ID="local-utils"

command -v magick >/dev/null || { echo "error: magick not found (imagemagick)" >&2; exit 1; }
[[ -f "$SRC" ]] || { echo "error: master not found: $SRC" >&2; exit 1; }

for s in 16 24 32 48 64 128 256 512; do
  install -d "$DIR/${s}x$s/apps"
  magick "$SRC" -resize ${s}x${s} "$DIR/${s}x$s/apps/$ID.png"
  echo "wrote $DIR/${s}x$s/apps/$ID.png"
done
