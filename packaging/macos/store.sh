#!/usr/bin/env bash
# Build a locally signed Mac App Store installer; never upload or install it.
set -euo pipefail

usage() {
  cat <<'USAGE'
usage: packaging/macos/store.sh --binary FILE --version X.Y.Z --build-number N
  --application-identity ID --installer-identity ID --profile FILE
  --icon FILE.icns --privacy-url HTTPS_URL --notices FILE --output FILE.pkg

Requires a release binary built with --features mac-app-store and
SCRANNOTATE_PRIVACY_URL set to the supplied public policy URL, plus macOS,
Apple signing tools, Python 3, production assets, and distribution credentials.
See docs/store-packaging.md. Existing output is never overwritten.
USAGE
}

fail() { echo "$*" >&2; exit 1; }
binary='' version='' build_number='' application_identity='' installer_identity=''
profile='' icon='' privacy_url='' notices='' output=''
while [[ $# -gt 0 ]]; do
  if [[ "$1" == --help ]]; then usage; exit 0; fi
  [[ $# -ge 2 && -n "$2" ]] || { usage >&2; exit 2; }
  case "$1" in
    --binary) binary="$2" ;;
    --version) version="$2" ;;
    --build-number) build_number="$2" ;;
    --application-identity) application_identity="$2" ;;
    --installer-identity) installer_identity="$2" ;;
    --profile) profile="$2" ;;
    --icon) icon="$2" ;;
    --privacy-url) privacy_url="$2" ;;
    --notices) notices="$2" ;;
    --output) output="$2" ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift 2
done
for argument in binary version build_number application_identity installer_identity profile icon privacy_url notices output; do
  [[ -n "${!argument}" ]] || fail "missing required option: ${argument//_/-}"
done
# Absolute paths also keep filenames beginning with '-' from becoming tool
# options, without depending on every Apple utility accepting a '--' delimiter.
for argument in binary profile icon notices output; do
  if [[ "${!argument}" != /* ]]; then
    printf -v "$argument" '%s' "$PWD/${!argument}"
  fi
done
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || fail 'version must be X.Y.Z without prerelease suffixes'
[[ "$build_number" =~ ^[1-9][0-9]{0,3}$ ]] || fail 'build-number must be an increasing integer from 1 to 9999'
[[ "$application_identity" != - && "$installer_identity" != - ]] || fail 'ad-hoc signing is not valid for the Store'
[[ "$application_identity" != "Developer ID"* && "$installer_identity" != "Developer ID"* ]] || fail 'use Mac App Store distribution identities, not Developer ID identities'
[[ "$output" == *.pkg ]] || fail 'output must end in .pkg'
[[ ! -e "$output" && ! -L "$output" ]] || fail "refusing to overwrite $output"
[[ "$icon" == *.icns ]] || fail 'icon must be a production .icns file'
project_root="$(cd "$(dirname "$0")/../.." && pwd)"
for input in "$binary" "$profile" "$icon" "$notices" "$project_root/LICENSE" "$project_root/NOTICE" "$project_root/PRIVACY.md"; do
  [[ -f "$input" && -s "$input" ]] || fail "missing or empty input: $input"
done
[[ "$(uname -s)" == Darwin ]] || fail 'Mac App Store packaging must run on macOS'
for tool in codesign security productbuild pkgutil iconutil lipo plutil python3; do
  command -v "$tool" >/dev/null || fail "required tool not found: $tool"
done

# Stage next to the final package, allowing atomic no-replace publication with
# a hard link once signing and verification have succeeded.
mkdir -p "$(dirname "$output")"
staging="$(mktemp -d "$(dirname "$output")/.scrannotate-store.XXXXXX")"
trap 'rm -rf "$staging"' EXIT
app="$staging/scrannotate.app"
security cms -D -i "$profile" -o "$staging/profile.plist"
python3 - "$project_root" "$binary" "$version" "$build_number" "$privacy_url" "$staging" <<'PY'
import datetime
import pathlib
import plistlib
import sys
import urllib.parse

root, binary, version, build, privacy, staging = sys.argv[1:]
root, staging = pathlib.Path(root), pathlib.Path(staging)
url = urllib.parse.urlsplit(privacy)
if url.scheme != "https" or not url.hostname or url.username or url.password or any(c.isspace() for c in privacy):
    sys.exit("privacy-url must be a public HTTPS URL without embedded credentials or whitespace")
data = pathlib.Path(binary).read_bytes()
if b"SCRANNOTATE_MAC_APP_STORE_BUILD=1" not in data:
    sys.exit("binary lacks the mac-app-store build marker; rebuild with --features mac-app-store")
if privacy.encode() not in data:
    sys.exit("privacy URL is not embedded in the binary; set SCRANNOTATE_PRIVACY_URL when building")
with (root / "packaging/macos/Info.plist").open("rb") as source:
    info = plistlib.load(source)
with (staging / "profile.plist").open("rb") as source:
    profile = plistlib.load(source)
with (root / "packaging/macos/store-entitlements.plist").open("rb") as source:
    entitlements = plistlib.load(source)
allowed = profile.get("Entitlements", {})
team = allowed.get("com.apple.developer.team-identifier")
app_id = allowed.get("com.apple.application-identifier", "")
if not team or team not in profile.get("TeamIdentifier", []):
    sys.exit("provisioning profile has no matching team identity")
if not app_id.endswith("." + info["CFBundleIdentifier"]) or "*" in app_id:
    sys.exit("provisioning profile must name this exact bundle identifier")
expires = profile.get("ExpirationDate")
if not expires or expires.replace(tzinfo=datetime.timezone.utc) <= datetime.datetime.now(datetime.timezone.utc):
    sys.exit("provisioning profile has expired")
if profile.get("ProvisionedDevices") or profile.get("ProvisionsAllDevices") or allowed.get("get-task-allow") or allowed.get("com.apple.security.get-task-allow"):
    sys.exit("use a Mac App Store distribution profile, not a development or Developer ID profile")
if not set(profile.get("Platform", [])).intersection({"OSX", "macOS"}):
    sys.exit("provisioning profile is not for macOS")
if not profile.get("DeveloperCertificates"):
    sys.exit("provisioning profile contains no allowed signing certificates")
entitlements.update({"com.apple.application-identifier": app_id, "com.apple.developer.team-identifier": team})
info.update({"CFBundleShortVersionString": version, "CFBundleVersion": build,
             "CFBundleIconFile": "scrannotate.icns", "ScrannotatePrivacyPolicyURL": privacy})
if not info.get("LSApplicationCategoryType"):
    sys.exit("Info.plist requires LSApplicationCategoryType before Store packaging")
for name, payload in (("Info.plist", info), ("entitlements.plist", entitlements)):
    with (staging / name).open("wb") as target:
        plistlib.dump(payload, target)
PY

# Validate the actual machine code and icon container with Apple's tools.
architectures="$(lipo -archs "$binary")"
for architecture in $architectures; do
  [[ "$architecture" == arm64 || "$architecture" == x86_64 ]] || fail "unsupported binary architecture: $architecture"
done
[[ -n "$architectures" ]] || fail 'binary has no supported architecture'
iconutil -c iconset -o "$staging/icon.iconset" "$icon"
[[ -s "$staging/icon.iconset/icon_512x512@2x.png" ]] || fail 'icon must include the 1024-pixel representation'
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/scrannotate"
chmod 755 "$app/Contents/MacOS/scrannotate"
cp "$staging/Info.plist" "$app/Contents/Info.plist"
cp "$profile" "$app/Contents/embedded.provisionprofile"
cp "$icon" "$app/Contents/Resources/scrannotate.icns"
cp "$project_root/LICENSE" "$project_root/NOTICE" "$project_root/PRIVACY.md" "$app/Contents/Resources/"
cp "$notices" "$app/Contents/Resources/THIRD_PARTY_NOTICES.txt"
plutil -lint "$app/Contents/Info.plist" "$staging/entitlements.plist"
# --timestamp: App Store Connect expects a secure timestamp on the application
# signature itself; productbuild only timestamps the installer around it.
codesign --force --timestamp --sign "$application_identity" --entitlements "$staging/entitlements.plist" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
codesign --display --extract-certificates "$staging/signer" "$app"
python3 - "$staging" <<'PY'
import pathlib
import plistlib
import sys
staging = pathlib.Path(sys.argv[1])
with (staging / "profile.plist").open("rb") as source:
    profile = plistlib.load(source)
if (staging / "signer0").read_bytes() not in profile["DeveloperCertificates"]:
    sys.exit("application signing certificate is not allowed by the provisioning profile")
PY
productbuild --component "$app" /Applications --sign "$installer_identity" "$staging/scrannotate.pkg"
pkgutil --check-signature "$staging/scrannotate.pkg"
ln "$staging/scrannotate.pkg" "$output"
echo "Created signed Store installer: $output"
