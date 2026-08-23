#!/usr/bin/env bash
#
# Local build helper for macOS. Builds a release binary and (on macOS)
# assembles/ad-hoc signs scrannotate.app the same way the release workflow
# does (packaging/macos/bundle.sh) — same behavior, just callable from your
# own machine.
#
# Windows and Linux binaries are built by CI (.github/workflows/release.yml
# on a version bump, or .github/workflows/ci.yml for every PR) since cross
# compiling Windows/Linux targets from macOS is not set up. On Windows or
# Linux, `cargo build --release` is all this script would do anyway.
#
# Usage:
#   ./build.sh                 # build + bundle for this Mac's architecture
#   ./build.sh --universal     # build + bundle a universal (arm64+x86_64) app
set -euo pipefail

root="$(cd "$(dirname "$0")" && pwd)"
cd "$root"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Not macOS: running a plain release build. Windows/Linux release" \
       "binaries are produced by CI (.github/workflows/release.yml)." >&2
  cargo build --release --locked
  exit 0
fi

universal=false
if [[ "${1:-}" == "--universal" ]]; then
  universal=true
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--universal]" >&2
  exit 2
fi

version="$(cargo metadata --format-version 1 --no-deps --locked | jq -r '.packages[] | select(.name == "scrannotate") | .version')"
dist="$root/dist"
mkdir -p "$dist"

if $universal; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  cargo build --release --locked --target aarch64-apple-darwin
  cargo build --release --locked --target x86_64-apple-darwin
  binary="$(mktemp -d)/scrannotate"
  lipo -create -output "$binary" \
    target/aarch64-apple-darwin/release/scrannotate \
    target/x86_64-apple-darwin/release/scrannotate
  archive="$dist/scrannotate-universal-apple-darwin.app.zip"
else
  cargo build --release --locked
  binary="target/release/scrannotate"
  archive="$dist/scrannotate-$(uname -m)-apple-darwin.app.zip"
fi

packaging/macos/bundle.sh "$binary" "$version" "$archive"
echo "Built $archive"
