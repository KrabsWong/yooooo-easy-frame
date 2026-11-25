#!/bin/zsh
set -euo pipefail

root="${0:A:h:h}"
binary="$root/target/debug/apng-builder"
app="$root/target/debug/Easy Frame.app"

[[ -x "$binary" ]] || { echo "Missing $binary; run cargo build --offline first." >&2; exit 1; }

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp -f "$binary" "$app/Contents/MacOS/apng-builder"
cp -f "$root/resources/Info.plist" "$app/Contents/Info.plist"
cp -f "$root/resources/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cp -f "$root/resources/bin/ffmpeg" "$app/Contents/Resources/ffmpeg"
cp -Rf "$root/resources/licenses" "$app/Contents/Resources/licenses"
chmod +x "$app/Contents/MacOS/apng-builder"
chmod +x "$app/Contents/Resources/ffmpeg"

codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true

echo "$app"
