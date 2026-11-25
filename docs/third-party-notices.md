# Third-party notices

Easy Frame bundles a minimal, statically linked FFmpeg 7.1.1 executable at
`Easy Frame.app/Contents/Resources/ffmpeg` for PNG sequence encoding.

FFmpeg is licensed under the GNU Lesser General Public License version 2.1 or
later. The license text is included in `resources/licenses/FFMPEG-LGPLv2.1.txt`.
The upstream source archive is available from
<https://ffmpeg.org/releases/ffmpeg-7.1.1.tar.xz> and its SHA-256 is
`733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1`.

The bundled executable is built by `scripts/build-ffmpeg.sh` with GPL and
version-3 components disabled. Its build configuration enables only the file
protocol, image2 demuxer, PNG decoder, APNG muxer and encoder, and the
`format`, `scale`, `split`, `palettegen`, `paletteuse`, and `swscale` filters required
by Easy Frame. Its SHA-256 is
`cf3a837b0ac0c39830c3219e20bb0a8e2b6a48e1a8b868a2832671364d4375d6`.
