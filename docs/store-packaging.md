# Preparing Store packages

These scripts prepare local packages from already-built release executables.
They do not upload, submit, install, or create developer-account identities.
Their output still needs native installation, permissions, capture, export,
upgrade, and store validation before submission. The existing portable release
workflow remains separate.

Supply real icons, published privacy-policy URLs, target-specific dependency
notices, and the identities registered to the app. No placeholder store assets
or signing credentials are supplied by this repository.

## Microsoft Store / MSIX

Run `packaging/windows/store.ps1` on Windows with PowerShell 5.1 or newer and
the Windows SDK. Provide the exact path to `makeappx.exe`; the script does not
download tools or guess an installed SDK version. Build with the MSVC target
matching the intended package:

```powershell
$env:SCRANNOTATE_PRIVACY_URL = 'https://your-published-domain/privacy'
cargo build --locked --release --target x86_64-pc-windows-msvc
python scripts/third_party_notices.py --target x86_64-pc-windows-msvc `
  --output target/THIRD_PARTY_NOTICES-windows-x64.txt
```

For ARM64, use `aarch64-pc-windows-msvc` and `-Architecture arm64`. The script
reads the PE machine header and rejects an architecture mismatch.

Create an asset directory containing these production PNGs at their exact
unscaled sizes. The script validates the PNG header and dimensions and copies
the supplied files; it does not create artwork.

| Filename | Dimensions |
| --- | --- |
| `StoreLogo.png` | 50 × 50 |
| `Square44x44Logo.png` | 44 × 44 |
| `Square150x150Logo.png` | 150 × 150 |

Use the identity name, publisher distinguished name, publisher display name,
and reserved app display name from Partner Center. Example invocation, after
setting the variables to the real values:

```powershell
./packaging/windows/store.ps1 `
  -Binary target/x86_64-pc-windows-msvc/release/scrannotate.exe `
  -Version $StoreVersion -Architecture x64 `
  -IdentityName $PartnerCenterIdentity -Publisher $PartnerCenterPublisher `
  -PublisherDisplayName $PublisherName -DisplayName $ReservedAppName `
  -AssetsDirectory $ProductionAssetDirectory `
  -Notices target/THIRD_PARTY_NOTICES-windows-x64.txt `
  -MakeAppxPath $WindowsSdkMakeAppx `
  -MaxVersionTested $ActuallyTestedWindowsVersion `
  -Output target/scrannotate-x64.msix
```

`StoreVersion` must be four integers, with a nonzero first component, a final
zero, and no component greater than 65535. For example, an app release
`0.4.0` could use a separately maintained Store version `1.0.400.0`; choose and
document a monotonically increasing mapping before the first submission.
Do not automatically copy the Cargo version into the package identity.
These checks follow Microsoft's [Store package and version requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements).

The default `MinVersion` is `10.0.19041.0`; raise it if the tested supported
baseline is newer. `MaxVersionTested` is required because the script cannot
infer which Windows builds have actually been tested. These values are package
policy inputs, not evidence of runtime compatibility.

The generated manifest declares a full-trust desktop app, the
`scrannotate.exe` [execution alias](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap5-appexecutionalias),
and the `Microsoft.VCLibs.140.00.UWPDesktop` runtime framework dependency for
MSVC builds. Test on a clean Windows machine without development tools;
verify the installed executable's DLL requirements if the toolchain changes.
Microsoft documents [framework dependency installation through the Store](https://learn.microsoft.com/en-us/windows/msix/msix-troubleshooting-guide).

The package contains `LICENSE`, `NOTICE`, `PRIVACY.md`, and
`THIRD_PARTY_NOTICES.txt`. MakeAppx runs its normal schema and semantic
validation with SHA-256 block hashes; validation is not disabled. Existing
output is refused, and temporary staging is removed on success or failure.
The result is an unsigned `.msix`; local sideload testing requires separate
test signing or an appropriate Store testing channel. Store-distributed MSIX
packages are signed by Microsoft, so a commercial code-signing certificate is
not required for this submission route. See the [Store signing guidance](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements).

Run the Windows App Certification Kit and installed-package tests before
submission. Explain the required `runFullTrust` capability in the submission.
Verify Start launch, execution-alias arguments/current directory, capture with
cursor on/off, clipboard persistence, Pictures redirection, upgrades, and
uninstall. This script does not create a multi-architecture bundle, symbol
upload package, localized resource index, or additional scale-specific assets.
Those can be added after validating the basic native packages; Microsoft's
[MakeAppx documentation](https://learn.microsoft.com/en-us/windows/msix/package/create-app-package-with-makeappx-tool)
describes package and bundle creation.

## Mac App Store

Run `packaging/macos/store.sh` on macOS with Apple's signing/packaging tools
and Python 3 available. Prepare:

- A release binary built with the `mac-app-store` feature, with capture enabled.
- A published HTTPS privacy-policy URL embedded at compile time using
  `SCRANNOTATE_PRIVACY_URL`.
- A production `.icns` containing its 1024-pixel representation.
- A current Mac App Store distribution provisioning profile for the exact
  bundle identifier in `packaging/macos/Info.plist`.
- The matching application-distribution identity and Mac App Store installer
  identity, with private keys available in the local keychain. Use the Store
  identity types, not Developer ID or ad-hoc signing.

Build each supported architecture with the same source and settings:

```sh
export SCRANNOTATE_PRIVACY_URL='https://your-published-domain/privacy'
cargo build --locked --release --features mac-app-store --target aarch64-apple-darwin
python3 scripts/third_party_notices.py --target aarch64-apple-darwin \
  --features mac-app-store --output target/THIRD_PARTY_NOTICES-macos-arm64.txt

cargo build --locked --release --features mac-app-store --target x86_64-apple-darwin
python3 scripts/third_party_notices.py --target x86_64-apple-darwin \
  --features mac-app-store --output target/THIRD_PARTY_NOTICES-macos-x64.txt

lipo -create target/aarch64-apple-darwin/release/scrannotate \
  target/x86_64-apple-darwin/release/scrannotate -output target/scrannotate-universal
cat target/THIRD_PARTY_NOTICES-macos-arm64.txt target/THIRD_PARTY_NOTICES-macos-x64.txt \
  > target/THIRD_PARTY_NOTICES-macos.txt
```

The script accepts a single architecture or a universal executable; test every
architecture included. For a single-architecture package, pass that executable
and its matching notices instead. Inspect `scrannotate --build-info` on a Mac
to confirm the build feature and exact privacy URL. The packaging script scans
the supplied binary for the Store build marker and policy URL; it does not
execute cross-architecture inputs. These checks prevent common mix-ups but do
not replace provenance or per-slice inspection of a universal executable.

```sh
packaging/macos/store.sh \
  --binary target/scrannotate-universal --version 0.4.0 --build-number 1 \
  --application-identity "$APP_STORE_APPLICATION_IDENTITY" \
  --installer-identity "$APP_STORE_INSTALLER_IDENTITY" \
  --profile "$DISTRIBUTION_PROFILE" --icon "$PRODUCTION_ICNS" \
  --privacy-url "$SCRANNOTATE_PRIVACY_URL" \
  --notices target/THIRD_PARTY_NOTICES-macos.txt \
  --output target/scrannotate-mac-store.pkg
```

Use an increasing build number from 1 to 9999; this script intentionally
supports a simple integer `CFBundleVersion`. The marketing version must be
three decimal components without prerelease suffixes.

The script validates profile expiry, platform, explicit App ID, team, and
distribution status, then derives the application/team entitlements from that
profile. It enables App Sandbox, user-selected file read/write, and app-scoped
security bookmarks. It embeds the profile, icon, privacy policy, license, and
notices in the bundle; sets version/icon metadata; signs the app; verifies its
signature; and checks the signing certificate against the profile's allowed
certificates. `productbuild` creates the signed installer and `pkgutil` checks
its signature. Manually check the printed installer certificate name against
the profile's developer team: the script verifies the installer signature but
does not independently validate that installer's team membership.
The script removes staging and refuses to replace an existing output package.

Apple describes the [profile/App ID relationship](https://developer.apple.com/documentation/technotes/tn3125-inside-code-signing-provisioning-profiles)
and the [signed component installer workflow](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution).
This preparation does not upload to App Store Connect or notarize a separate
Developer ID distribution.

Before submission, validate the actual signed package and test the supported
OS versions and architectures through the appropriate Apple distribution test
workflow. In particular, verify Screen Recording consent/denial, capture after
relaunch, folder authorization and bookmark persistence, file import, clipboard,
updates, icons, and the live in-app privacy link. A valid signature alone does
not establish Store acceptance or correct sandbox behavior.

## Validation available on Linux

Shell syntax, ShellCheck, plist parsing, and mocked native-tool boundary checks
can run here. They exercise input rejection, required documents/assets,
temporary cleanup, profile validation, and packaging command arguments. They
do not execute Apple's signing tools or Windows MakeAppx, and cannot establish
native package installability or store validation.

```sh
bash -n packaging/macos/store.sh
shellcheck packaging/macos/store.sh
python3 -m unittest discover -s scripts -p 'test_store_packaging.py' -v
```

The Windows control-flow test runs when `pwsh` is on `PATH`; it explicitly
substitutes the platform guard in a temporary copy and uses a fake MakeAppx.
The Apple test likewise uses fake signing and packaging tools. Neither test
requires or touches signing keys, and missing PowerShell is reported as a skip.
