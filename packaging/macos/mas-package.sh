#!/usr/bin/env bash
#
# Build a Mac App Store submission package (.pkg) for scrannotate.
#
# This is a DIFFERENT signing path from packaging/macos/bundle.sh:
#   bundle.sh  -> direct download, ad-hoc (or Developer ID) signed .app.zip
#   this file  -> Mac App Store, sandboxed .app signed with the "3rd Party
#                 Mac Developer Application" cert, wrapped in a .pkg signed
#                 with the "3rd Party Mac Developer Installer" cert, ready to
#                 upload to App Store Connect.
#
# Usage:
#   packaging/macos/mas-package.sh <binary> <version> <output.pkg>
#
# Required environment (see docs/SIGNING.md):
#   MAS_APP_CERT        e.g. "3rd Party Mac Developer Application: AppCove, Inc. (TEAMID)"
#   MAS_INSTALLER_CERT  e.g. "3rd Party Mac Developer Installer: AppCove, Inc. (TEAMID)"
#   MAS_PROVISION       path to the App Store provisioning profile (.provisionprofile)
#
# The bundle must carry an app icon (AppIcon.icns) and the MAS entitlements;
# both live beside this script once you add the icon (see docs/SIGNING.md).
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: $0 <binary> <version> <output.pkg>" >&2
  exit 2
fi

binary="$1"
version="$2"
output="$3"
here="$(cd "$(dirname "$0")" && pwd)"

for var in MAS_APP_CERT MAS_INSTALLER_CERT MAS_PROVISION; do
  if [[ -z "${!var:-}" ]]; then
    echo "error: \$$var is not set — see docs/SIGNING.md" >&2
    exit 1
  fi
done
if [[ ! -f "$MAS_PROVISION" ]]; then
  echo "error: MAS_PROVISION=$MAS_PROVISION not found" >&2
  exit 1
fi
entitlements="$here/mas.entitlements"

staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
app="$staging/scrannotate.app"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/scrannotate"
cp "$here/Info.plist" "$app/Contents/Info.plist"
for key in CFBundleShortVersionString CFBundleVersion; do
  /usr/libexec/PlistBuddy -c "Set :$key $version" "$app/Contents/Info.plist"
done
# App Store builds must embed the provisioning profile and an icon.
cp "$MAS_PROVISION" "$app/Contents/embedded.provisionprofile"
if [[ -f "$here/AppIcon.icns" ]]; then
  cp "$here/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
else
  echo "warning: $here/AppIcon.icns missing — the Store requires an icon" >&2
fi

# Sign the app with the sandbox entitlements, then wrap + sign the installer.
codesign --force --timestamp --options runtime \
  --entitlements "$entitlements" \
  --sign "$MAS_APP_CERT" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

mkdir -p "$(dirname "$output")"
productbuild --component "$app" /Applications \
  --sign "$MAS_INSTALLER_CERT" "$output"

echo "Built $output"
echo "Upload with:  xcrun altool --upload-app -f \"$output\" -t macos \\"
echo "                --apiKey \$ASC_KEY_ID --apiIssuer \$ASC_ISSUER_ID"
echo "(or drag it into Transporter.app). See docs/SIGNING.md."
