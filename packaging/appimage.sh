#!/usr/bin/env bash
# Build a BeatByte AppImage (run on Linux x86_64).
#
# Usage: packaging/appimage.sh <target-triple> <version>
#   e.g. packaging/appimage.sh x86_64-unknown-linux-gnu 0.6.0
#
# Downloads appimagetool if not present. Produces
# dist/BeatByte-<version>-x86_64.AppImage

set -euo pipefail

TARGET="${1:?target triple}"
VERSION="${2:?version}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/$TARGET/release/beatbyte"
OUT="$ROOT/dist"
APPDIR="$OUT/BeatByte.AppDir"

[ -f "$BIN" ] || { echo "missing binary: $BIN" >&2; exit 1; }

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"

cp "$BIN" "$APPDIR/usr/bin/beatbyte"
cp -R "$ROOT/assets" "$APPDIR/usr/bin/assets"

python3 "$ROOT/packaging/make-icon.py"
cp "$ROOT/packaging/icon.png" "$APPDIR/beatbyte.png"

cat > "$APPDIR/beatbyte.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=BeatByte
Comment=An original 8-bit rhythm game
Exec=beatbyte
Icon=beatbyte
Categories=Game;
Terminal=false
DESKTOP

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/beatbyte" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

TOOL="$OUT/appimagetool"
if [ ! -x "$TOOL" ]; then
  curl -sL -o "$TOOL" \
    "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
  chmod +x "$TOOL"
fi

APPIMAGE="$OUT/BeatByte-${VERSION}-x86_64.AppImage"
rm -f "$APPIMAGE"
# --appimage-extract-and-run avoids needing FUSE on CI runners.
ARCH=x86_64 "$TOOL" --appimage-extract-and-run "$APPDIR" "$APPIMAGE" >/dev/null
echo "built $APPIMAGE"
