#!/usr/bin/env bash
# Build an AppImage from a release binary.
# usage: packaging/linux/appimage.sh <photo-studio binary> <version> <arch: x86_64|aarch64> <out dir>
set -euo pipefail
BIN="$1"; VERSION="$2"; ARCH="$3"; OUT="$4"
HERE="$(cd "$(dirname "$0")" && pwd)"
APPDIR="$(mktemp -d)/PhotoStudio.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps" "$APPDIR/usr/share/metainfo"
cp "$BIN" "$APPDIR/usr/bin/photo-studio"
cp "$HERE/photo-studio.desktop" "$APPDIR/usr/share/applications/"
cp "$HERE/photo-studio.desktop" "$APPDIR/"
cp "$HERE/photo-studio.metainfo.xml" "$APPDIR/usr/share/metainfo/com.devopsmonk.PhotoStudio.appdata.xml"
cp "$HERE/../icons/256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/photo-studio.png"
cp "$HERE/../icons/256.png" "$APPDIR/photo-studio.png"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
exec "$HERE/usr/bin/photo-studio" "$@"
RUN
chmod +x "$APPDIR/AppRun"
TOOL="appimagetool-$(uname -m).AppImage"
if [ ! -x "$TOOL" ]; then
  curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$(uname -m).AppImage"
  chmod +x "$TOOL"
fi
ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "./$TOOL" --no-appstream "$APPDIR" "$OUT/photo-studio-$VERSION-linux-$ARCH.AppImage"
