#!/usr/bin/env bash
# Build a local ad-hoc signed arm64 DMG with Sparkle enabled.
set -euo pipefail
cd "$(dirname "$0")/.."

: "${SPARKLE_PUBLIC_ED_KEY:?Set SPARKLE_PUBLIC_ED_KEY to the release signing public key}"
export SPARKLE_PUBLIC_ED_KEY

bash scripts/package.sh

APP="dist/Easy Frame.app"
STAGING="dist/dmg/staging"
DMG="dist/Easy Frame.dmg"

rm -rf "dist/dmg" "$DMG"
mkdir -p "$STAGING"

ditto "$APP" "$STAGING/Easy Frame.app"
ln -s /Applications "$STAGING/Applications"
hdiutil create -volname "Easy Frame" -srcfolder "$STAGING" -ov -format UDZO "$DMG"
hdiutil verify "$DMG"

echo "==> built $DMG (ad-hoc signed, not notarized, Sparkle enabled)"
