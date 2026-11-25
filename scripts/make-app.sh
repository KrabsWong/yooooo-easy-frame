#!/bin/zsh
set -euo pipefail

root="${0:A:h:h}"
binary="$root/target/debug/apng-builder"
app="$root/target/debug/Easy Frame.app"

[[ -x "$binary" ]] || { echo "Missing $binary; run cargo build --offline first." >&2; exit 1; }

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp -f "$binary" "$app/Contents/MacOS/apng-builder"
cp -f "$root/resources/Info.plist" "$app/Contents/Info.plist"
chmod +x "$app/Contents/MacOS/apng-builder"

codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true

echo "$app"
