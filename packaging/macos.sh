#!/usr/bin/env bash
# Build BeatByte.app and a distributable DMG (run on macOS).
#
# Usage: packaging/macos.sh <target-triple> <version>
#   e.g. packaging/macos.sh aarch64-apple-darwin 0.6.0
#        packaging/macos.sh x86_64-apple-darwin 0.18.40   # Intel Macs
#        packaging/macos.sh universal-apple-darwin 0.18.40
#
# Expects target/<triple>/release/beatbyte to exist; `universal`
# expects BOTH target/aarch64-apple-darwin and target/x86_64-apple-darwin
# and joins them with lipo. beatbyte-cli goes into the bundle beside the
# game when it was built (the sync lives there, ADR-0021). The minimum
# macOS the bundle claims is MACOSX_DEPLOYMENT_TARGET — set the same
# value when building, or the claim and the binary disagree (default
# 11.0, the oldest macOS the arm64 build can target anyway).
# Produces dist/BeatByte-<version>-<triple>.dmg

set -euo pipefail

TARGET="${1:?target triple}"
VERSION="${2:?version}"
MIN_MACOS="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/dist"
APP="$OUT/BeatByte.app"

# The binary <name> for $TARGET, joined across both architectures for
# universal. Local builds without --target land in target/release.
binary() {
  local name="$1"
  if [ "$TARGET" = "universal-apple-darwin" ]; then
    local arm="$ROOT/target/aarch64-apple-darwin/release/$name"
    local intel="$ROOT/target/x86_64-apple-darwin/release/$name"
    [ -f "$arm" ] && [ -f "$intel" ] || return 1
    mkdir -p "$OUT/lipo"
    lipo -create "$arm" "$intel" -output "$OUT/lipo/$name"
    echo "$OUT/lipo/$name"
    return 0
  fi
  local bin="$ROOT/target/$TARGET/release/$name"
  [ -f "$bin" ] || bin="$ROOT/target/release/$name"
  [ -f "$bin" ] || return 1
  echo "$bin"
}

BIN="$(binary beatbyte)" || { echo "missing binary beatbyte for $TARGET" >&2; exit 1; }

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp "$BIN" "$APP/Contents/MacOS/beatbyte"
if CLI="$(binary beatbyte-cli)"; then
  cp "$CLI" "$APP/Contents/MacOS/beatbyte-cli"
  # A script in Contents/MacOS is signed through extended attributes,
  # which a plain copy (rsync, zip) drops — the bundle then fails
  # verification. Resources is not a code location.
  cp "$ROOT/tools/play-synced.sh" "$APP/Contents/Resources/play-synced.sh"
fi
cp -R "$ROOT/assets" "$APP/Contents/Resources/assets"

cp "$ROOT/packaging/icons/macos/BeatByte.icns" \
  "$APP/Contents/Resources/BeatByte.icns"

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
  <key>NSMicrophoneUsageDescription</key><string>The two monitors on the stage's PA stacks show the tempo and the level the microphone hears. Nothing is recorded or sent.</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>${MIN_MACOS}</string>
</dict>
</plist>
PLIST

# Ad-hoc signature so Gatekeeper shows the normal unidentified-developer
# flow instead of refusing outright.
codesign --force --deep -s - "$APP"
rm -rf "$OUT/lipo"

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
# GitHub's macOS runners intermittently fail hdiutil with a SPURIOUS
# "No space left on device" (df showed 95 GiB free at the moment of
# failure — a known diskimages-helper flake, not actual disk
# pressure). Retrying is the community-standard mitigation.
attempts=0
until hdiutil create -volname "BeatByte" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null; do
  attempts=$((attempts + 1))
  if [ "$attempts" -ge 6 ]; then
    echo "hdiutil failed $attempts times, giving up" >&2
    exit 1
  fi
  echo "hdiutil attempt $attempts failed; retrying in 5s" >&2
  sleep 5
done
echo "built $DMG"
