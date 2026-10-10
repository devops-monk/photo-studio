#!/usr/bin/env bash
# Build Photo Studio.app and a .dmg from a (universal) binary.
# usage: packaging/macos/bundle.sh <photo-studio binary> <version> <out dir>
set -euo pipefail
BIN="$1"; VERSION="$2"; OUT="$3"
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$OUT/Photo Studio.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/photo-studio"
sed "s/__VERSION__/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
iconutil -c icns "$HERE/AppIcon.iconset" -o "$APP/Contents/Resources/AppIcon.icns"
# Ad-hoc signature so Apple Silicon will run it; see README for Gatekeeper notes.
codesign --force --deep --sign - "$APP"
DMG="$OUT/photo-studio-$VERSION-macos-universal.dmg"
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
# hdiutil intermittently fails with "Resource busy" on CI runners; retry a few times.
for attempt in 1 2 3 4 5; do
  if hdiutil create -volname "Photo Studio $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null; then
    break
  fi
  [ "$attempt" = 5 ] && exit 1
  sleep $((attempt * 5))
done
rm -rf "$STAGE"
echo "$DMG"
