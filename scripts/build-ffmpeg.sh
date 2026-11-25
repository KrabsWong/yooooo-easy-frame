#!/usr/bin/env bash
# Build the pinned minimal LGPL FFmpeg executable bundled with Easy Frame.
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=7.1.1
ARCHIVE_SHA256=733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1
BINARY_SHA256=cf3a837b0ac0c39830c3219e20bb0a8e2b6a48e1a8b868a2832671364d4375d6
ROOT="$PWD"
SOURCE_URL="https://ffmpeg.org/releases/ffmpeg-${VERSION}.tar.xz"
BUILD_DIR="$ROOT/build/ffmpeg-${VERSION}"
ARCHIVE="$BUILD_DIR/ffmpeg-${VERSION}.tar.xz"
SOURCE="$BUILD_DIR/ffmpeg-${VERSION}"
OUTPUT="$ROOT/resources/bin/ffmpeg"

mkdir -p "$BUILD_DIR" "$(dirname "$OUTPUT")"
if [ ! -f "$ARCHIVE" ]; then
  curl --fail --location --retry 3 "$SOURCE_URL" --output "$ARCHIVE.download"
  mv "$ARCHIVE.download" "$ARCHIVE"
fi
printf '%s  %s\n' "$ARCHIVE_SHA256" "$ARCHIVE" | shasum -a 256 --check

rm -rf "$SOURCE" "$BUILD_DIR/build"
tar -xf "$ARCHIVE" -C "$BUILD_DIR"
mkdir -p "$BUILD_DIR/build"
cd "$BUILD_DIR/build"

MACOSX_DEPLOYMENT_TARGET=14.0 ../ffmpeg-${VERSION}/configure \
  --disable-shared --enable-static --disable-autodetect --disable-everything \
  --disable-doc --disable-debug --disable-ffplay --disable-ffprobe \
  --disable-network --disable-iconv --disable-sdl2 --disable-xlib \
  --disable-libxcb --disable-vulkan --disable-gpl --disable-version3 \
  --disable-nonfree --enable-small --enable-ffmpeg --enable-zlib \
  --enable-protocol=file --enable-demuxer=image2 --enable-decoder=png \
  --enable-parser=png --enable-muxer=apng --enable-encoder=apng \
  --enable-filter=format --enable-filter=scale --enable-filter=split --enable-filter=palettegen \
  --enable-filter=paletteuse --enable-swscale --cc=clang --arch=arm64 \
  --target-os=darwin --extra-cflags='-mmacosx-version-min=14.0' \
  --extra-ldflags='-mmacosx-version-min=14.0'
make -j"$(sysctl -n hw.ncpu)" ffmpeg
strip ffmpeg
printf '%s  %s\n' "$BINARY_SHA256" "$PWD/ffmpeg" | shasum -a 256 --check
cp -f "$PWD/ffmpeg" "$OUTPUT"
chmod +x "$OUTPUT"

cd "$SOURCE"
cp -f COPYING.LGPLv2.1 "$ROOT/resources/licenses/FFMPEG-LGPLv2.1.txt"
cp -f LICENSE.md "$ROOT/resources/licenses/FFMPEG-LICENSE.txt"

echo "==> built bundled FFmpeg $VERSION at $OUTPUT"
