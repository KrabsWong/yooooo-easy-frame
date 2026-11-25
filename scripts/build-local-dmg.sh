#!/usr/bin/env bash
# Build a local ad-hoc signed arm64 DMG without Sparkle updates.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$(awk -F'"' '$1 == "version = " { print $2; exit }' Cargo.toml)"
TARGET="aarch64-apple-darwin"
APP="dist/local-dmg/Easy Frame.app"
STAGING="dist/local-dmg/staging"
DMG="dist/Easy Frame.dmg"

[[ -n "$VERSION" ]] || { echo "Missing package version" >&2; exit 1; }
[[ -x "target/$TARGET/release/apng-builder" ]] || {
  echo "Missing release binary; run cargo build --locked --release --target $TARGET" >&2
  exit 1
}

rm -rf dist/local-dmg "$DMG"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$STAGING"

cp "target/$TARGET/release/apng-builder" "$APP/Contents/MacOS/apng-builder"
cp resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp resources/Info.plist "$APP/Contents/Info.plist"
cp resources/bin/ffmpeg "$APP/Contents/Resources/ffmpeg"
ditto resources/licenses "$APP/Contents/Resources/licenses"
chmod +x "$APP/Contents/Resources/ffmpeg"

/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$APP/Contents/Info.plist"

codesign --force --sign - "$APP/Contents/Resources/ffmpeg"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"

ditto "$APP" "$STAGING/Easy Frame.app"
ln -s /Applications "$STAGING/Applications"
hdiutil create -volname "Easy Frame" -srcfolder "$STAGING" -ov -format UDZO "$DMG"
hdiutil verify "$DMG"

echo "==> built $DMG (ad-hoc signed, not notarized, updater disabled)"
