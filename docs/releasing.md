# Releasing Easy Frame

Easy Frame uses Sparkle 2.9.3 for updates. Releases are Apple Silicon only,
target macOS 14+, and are ad-hoc signed. There is no Developer ID certificate,
notarization, Developer Portal account, or private key in this repository.

Each app bundle includes a pinned minimal LGPL FFmpeg build at
`Contents/Resources/ffmpeg`, produced by `scripts/build-ffmpeg.sh` and verified
by SHA-256 before packaging. Users do not need a system FFmpeg installation.
License and source details are in `docs/third-party-notices.md`.

The stable feed is:

```text
https://github.com/KrabsWong/yooooo-easy-frame/releases/latest/download/appcast.xml
```

Every stable release must contain `appcast.xml`. Do not mark a release without
that asset as Latest, and do not replace an already published release.

## One-time configuration

Generate a permanent Sparkle Ed25519 key for this app. Fetch the pinned SDK
with `bash scripts/fetch-sparkle.sh`, then use its key generator:

```sh
build/sparkle/sdk/bin/generate_keys --account easy-frame
```

Store the printed public key as the Actions **variable**
`SPARKLE_PUBLIC_ED_KEY`. Store the exported 32-byte seed as the Actions
**secret** `SPARKLE_PRIVATE_ED_KEY`. Keep an encrypted backup of the seed; all
releases must use the same key pair. Add `OPENCODE_GO_API_KEY` as an Actions
secret and `OPENCODE_MODEL` as an Actions variable for release-note drafting,
for example `opencode-go/minimax-m3`.

The release helper derives the public key from the secret before signing and
stops if it does not match the bundled key. It passes the seed to Sparkle's
signer over stdin and never writes it to a file or command line.

## Release flow

Start a release from current `main` with a request tag:

```sh
git tag release/patch/20261125-1
git push origin refs/tags/release/patch/20261125-1
```

`release/patch/<id>`, `release/minor/<id>`, and `release/major/<id>` request a
version increment. Actions can also be started manually from `main` with a
level selector. The workflow calculates the next version from the latest
GitHub release, opens a version-only PR touching only `Cargo.toml` and
`Cargo.lock`, validates its exact contents, and merges it automatically.
Additional required checks can block that merge; resolve them and rerun the
original Actions run.

The merge commit is tagged `vX.Y.Z`. Do not push formal version tags yourself.
The same run opens a `release-notes/<tag>` PR with a user-facing summary.
Review the rendered Markdown, request changes with `/oc <instructions>` if
needed, then approve the current PR head with:

```text
/release-notes approve <full-head-sha>
```

Wait for the bot acknowledgement, then merge the PR manually. Any later
commit or revision request revokes the approval. The release job verifies the
approval before building from the original tag commit.

The macOS job runs tests, builds the arm64 app with the Sparkle feature,
verifies updater preferences, signs nested Sparkle code and the app ad-hoc,
creates a draft GitHub Release, uploads the ZIP and signed appcast, and marks
the release Latest only after both uploads succeed. `dist/Easy-Frame-<version>-macOS-arm64.zip`
and `dist/appcast.xml` are the published assets.

Do not bypass the release-note approval status on `main`. Closing an unmerged
notes PR does not publish. Release jobs are serialized to prevent stale notes
from being published.

## Installation

Users should extract the ZIP and move `Easy Frame.app` into Applications with
Finder before opening it. An ad-hoc signed download may need approval in
System Settings -> Privacy & Security -> Open Anyway on first launch. Do not
instruct users to disable Gatekeeper.

## Local asset preparation

Set `SPARKLE_PUBLIC_ED_KEY`, run `bash scripts/package.sh`, then provide
`SPARKLE_PRIVATE_ED_KEY` in the environment and run:

```sh
python3 scripts/release.py --tag v0.2.0
```

The bridge smoke test can be run after fetching the SDK:

```sh
python3 scripts/test-updater.py --sdk build/sparkle/sdk
```
