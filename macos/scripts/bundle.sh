#!/bin/sh
# Assemble ClaudeUsageWidget.app from a built binary and ad-hoc sign it.
#
# Usage: macos/scripts/bundle.sh <binary> <out_dir> <version>
# Run from the repository root on macOS. Produces <out_dir>/ClaudeUsageWidget.app.
set -eu

BIN=$1
OUT=$2
VERSION=$3
APP="$OUT/ClaudeUsageWidget.app"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/ClaudeUsageWidget"
sed "s/__VERSION__/$VERSION/g" macos/Info.plist > "$APP/Contents/Info.plist"

# App icon, generated from the same artwork as the Windows icon.
WORK=$(mktemp -d)
sips -s format png windows/assets/icon.ico --out "$WORK/icon.png" >/dev/null
SET="$WORK/AppIcon.iconset"
mkdir "$SET"
for size in 16 32 128 256; do
    sips -z $size $size "$WORK/icon.png" --out "$SET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$WORK/icon.png" --out "$SET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$WORK"

# Ad-hoc signature ("-"): no Apple Developer ID, but satisfies Apple Silicon's
# requirement that all code is signed. Gatekeeper still asks the user to approve it.
codesign --force --deep --sign - "$APP"
echo "built $APP"
