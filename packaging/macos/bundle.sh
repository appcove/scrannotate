#!/usr/bin/env bash
#
# Assemble, ad-hoc sign, and zip scrannotate.app.
#
# Usage: packaging/macos/bundle.sh <binary> <version> <archive> <notices>
#
#   binary   compiled scrannotate executable to wrap
#   version  stamped into CFBundleShortVersionString and CFBundleVersion
#   archive  destination .app.zip (parent directory is created)
#   notices  generated target-specific THIRD_PARTY_NOTICES.txt
#
# The bundle exists so macOS attributes the Screen Recording grant to
# scrannotate's own bundle identity rather than to whatever launched it.
# ditto, not zip: it preserves the signature and the executable bit.
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <binary> <version> <archive> <notices>" >&2
  exit 2
fi

binary="$1"
version="$2"
archive="$3"
notices="$4"
project_root="$(cd "$(dirname "$0")/../.." && pwd)"

for document in "$project_root/LICENSE" "$project_root/NOTICE" "$project_root/PRIVACY.md" "$notices"; do
  if [[ ! -s "$document" ]]; then
    echo "missing distribution document: $document" >&2
    exit 1
  fi
done

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
app="$staging/scrannotate.app"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/scrannotate"
cp "$(dirname "$0")/Info.plist" "$app/Contents/Info.plist"
cp "$project_root/LICENSE" "$project_root/NOTICE" "$project_root/PRIVACY.md" "$app/Contents/Resources/"
cp "$notices" "$app/Contents/Resources/THIRD_PARTY_NOTICES.txt"
for key in CFBundleShortVersionString CFBundleVersion; do
  /usr/libexec/PlistBuddy -c "Set :$key $version" "$app/Contents/Info.plist"
done

# arm64 refuses to run unsigned; ad-hoc is as far as we go without a
# Developer ID, so Gatekeeper still needs the README-documented unlock.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

mkdir -p "$(dirname "$archive")"
ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
