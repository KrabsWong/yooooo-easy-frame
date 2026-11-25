#!/usr/bin/env bash
# Build the distributable arm64 app with Sparkle and an ad-hoc signature.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${SPARKLE_PUBLIC_ED_KEY:?Set SPARKLE_PUBLIC_ED_KEY to the release signing public key}"
export SPARKLE_PUBLIC_ED_KEY
python3 - <<'PY'
import base64, os
key = base64.b64decode(os.environ['SPARKLE_PUBLIC_ED_KEY'], validate=True)
if len(key) != 32:
    raise SystemExit('SPARKLE_PUBLIC_ED_KEY must encode a 32-byte Ed25519 public key')
PY
SDK="${SPARKLE_SDK_DIR:-}"
if [ -z "$SDK" ]; then SDK="$(bash scripts/fetch-sparkle.sh)"; fi
SDK="$(cd "$SDK" && pwd)"
export SPARKLE_FRAMEWORK_DIR="$SDK/Sparkle.xcframework/macos-arm64_x86_64"
export MACOSX_DEPLOYMENT_TARGET=14.0
APP="dist/Easy Frame.app"
BIN="target/aarch64-apple-darwin/release/apng-builder"
mkdir -p build
cargo build --locked --release --target aarch64-apple-darwin --features sparkle
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp "$BIN" "$APP/Contents/MacOS/apng-builder"
cp resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp resources/bin/ffmpeg "$APP/Contents/Resources/ffmpeg"
ditto resources/licenses "$APP/Contents/Resources/licenses"
chmod +x "$APP/Contents/Resources/ffmpeg"
cp resources/Info.plist "$APP/Contents/Info.plist"
ditto "$SPARKLE_FRAMEWORK_DIR/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"
python3 - <<'PY'
import json, os, plistlib, subprocess
from pathlib import Path
metadata = json.loads(subprocess.check_output(['cargo','metadata','--no-deps','--format-version','1']))
version = next(p['version'] for p in metadata['packages'] if p['name'] == 'apng-builder')
p = Path('dist/Easy Frame.app/Contents/Info.plist')
info = plistlib.loads(p.read_bytes())
info.update(CFBundleShortVersionString=version, CFBundleVersion=version,
    SUFeedURL='https://github.com/KrabsWong/yooooo-easy-frame/releases/latest/download/appcast.xml',
    SUPublicEDKey=os.environ['SPARKLE_PUBLIC_ED_KEY'], SUEnableAutomaticChecks=True,
    SUScheduledCheckInterval=43200, SUAutomaticallyUpdate=False, SUAllowsAutomaticUpdates=False,
    SUVerifyUpdateBeforeExtraction=True, SURequireSignedFeed=True,
    SUSignedFeedFailureExpirationInterval=0)
p.write_bytes(plistlib.dumps(info))
PY
# Sign nested executable code from the inside out, then the enclosing bundle.
FRAMEWORK="$APP/Contents/Frameworks/Sparkle.framework"
for service in "$FRAMEWORK"/Versions/B/XPCServices/*.xpc; do
  codesign --force --sign - "$service"
done
codesign --force --sign - "$FRAMEWORK/Versions/B/Autoupdate"
codesign --force --sign - "$FRAMEWORK/Versions/B/Updater.app"
codesign --force --sign - "$FRAMEWORK"
codesign --force --sign - "$APP/Contents/Resources/ffmpeg"
codesign --force --sign - "$APP"
codesign --verify --deep --strict "$APP"
echo "==> built $APP (ad-hoc signed, not notarized)"
if [ "${1:-}" = "--run" ]; then open "$APP"; fi
