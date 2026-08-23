#!/usr/bin/env bash
# Build BeatByte.app and a distributable DMG (run on macOS).
#
# Usage: packaging/macos.sh <target-triple> <version>
#   e.g. packaging/macos.sh aarch64-apple-darwin 0.6.0
#
# Expects target/<triple>/release/beatbyte to exist. Produces
# dist/BeatByte-<version>-<triple>.dmg

set -euo pipefail

TARGET="${1:?target triple}"
VERSION="${2:?version}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/$TARGET/release/beatbyte"
# Local builds without --target land in target/release.
[ -f "$BIN" ] || BIN="$ROOT/target/release/beatbyte"
OUT="$ROOT/dist"
APP="$OUT/BeatByte.app"

[ -f "$BIN" ] || { echo "missing binary: $BIN" >&2; exit 1; }

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BIN" "$APP/Contents/MacOS/beatbyte"
cp -R "$ROOT/assets" "$APP/Contents/Resources/assets"

# Icon: PNG → iconset → icns.
python3 "$ROOT/packaging/make-icon.py"
ICONSET="$OUT/BeatByte.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for size in 16 32 64 128 256 512; do
  sips -z "$size" "$size" "$ROOT/packaging/icon.png" \
    --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$ROOT/packaging/icon.png" \
    --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/BeatByte.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>BeatByte</string>
  <key>CFBundleDisplayName</key><string>BeatByte</string>
  <key>CFBundleIdentifier</key><string>io.github.pepperonas.beatbyte</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>beatbyte</string>
  <key>CFBundleIconFile</key><string>BeatByte</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
</dict>
</plist>
PLIST

# Ad-hoc signature so Gatekeeper shows the normal unidentified-developer
# flow instead of refusing outright.
codesign --force --deep -s - "$APP"

# CI runners (especially arm64 macOS) run out of disk during hdiutil —
# the build tree is no longer needed once the binary is inside the
# .app, so reclaim it there. Never touch a developer's target/.
if [ "${CI:-}" = "true" ]; then
  df -h / || true
  rm -rf "$ROOT/target"
  df -h / || true
fi

DMG="$OUT/BeatByte-${VERSION}-${TARGET}.dmg"
rm -f "$DMG"
hdiutil create -volname "BeatByte" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null
echo "built $DMG"
