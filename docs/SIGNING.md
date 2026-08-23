# Signing & Store publishing

How scrannotate is signed and how to publish it to the **Mac App Store** and
the **Microsoft Store**. Everything the automation can do is already scripted;
this document is the list of accounts, certificates, identity values, and CI
secrets **you** must provide, because they are paid and account-bound and
cannot be scripted or committed.

> For the **click-by-click ordered walkthrough** of both portals (register,
> build, upload, fill the listing, submit), see
> [PUBLISHING.md](PUBLISHING.md). This file is the reference of what goes
> *into* those steps; that file is the sequence.

> **Status:** the scripts and manifests exist and are wired into an opt-in CI
> workflow (`.github/workflows/store-release.yml`), but they stay dormant
> until the secrets below are set. Nothing here signs or uploads on its own.

## Prerequisite for both stores: an app icon

scrannotate currently ships **no icon**. Both stores require one:

- **macOS**: `AppIcon.icns` at `packaging/macos/AppIcon.icns` (build with
  `iconutil` from an `.iconset`, or Icon Composer). Referenced by
  `mas-package.sh`.
- **Windows**: the PNG tile set in `packaging/windows/Assets/` — see that
  folder's `README.md` for sizes.

Design one 1024×1024 master and generate both sets from it.

---

## Mac App Store

Sandboxed `.pkg`, uploaded to App Store Connect. This is a **separate signing
path** from the direct-download `.app.zip` that `packaging/macos/bundle.sh`
already produces — do not confuse the two.

### What you need

1. **Apple Developer Program** membership ($99/yr) — <https://developer.apple.com>.
2. In App Store Connect, **register the app** with bundle id
   `com.appcove.scrannotate` (matches `packaging/macos/Info.plist`) and create
   the app record.
3. Two signing certificates (Certificates, Identifiers & Profiles):
   - **3rd Party Mac Developer Application** — signs the `.app`.
   - **3rd Party Mac Developer Installer** — signs the `.pkg`.
4. A **Mac App Store provisioning profile** for the app id, downloaded as a
   `.provisionprofile`.
5. An **App Store Connect API key** (Users and Access → Integrations) for
   headless upload: the `.p8` key file, its Key ID, and Issuer ID.

### Sandbox caveats (read before you rely on approval)

`packaging/macos/mas.entitlements` enables the App Sandbox plus the
Pictures-folder entitlement, because Ctrl+S writes a PNG to
`~/Pictures/Screenshots` with **no save dialog** — the sandbox forbids that
without either the Pictures entitlement (what we use) or a user-selected save
panel. If review objects to silent Pictures writes, switch Ctrl+S to an
`NSSavePanel` and swap the entitlement for
`com.apple.security.files.user-selected.read-write`.

Screen Recording needs **no entitlement** — ScreenCaptureKit is gated at
runtime by the user's TCC grant, which works inside the sandbox. Apple does
allow screen-capture apps on the Mac App Store, but expect review scrutiny of
the capture permission prompt and its usage-description string
(`NSScreenCaptureUsageDescription` in `Info.plist`).

### Build & upload

```bash
# Sign vars come from the certs above (exact names as they appear in Keychain):
export MAS_APP_CERT="3rd Party Mac Developer Application: AppCove, Inc. (TEAMID)"
export MAS_INSTALLER_CERT="3rd Party Mac Developer Installer: AppCove, Inc. (TEAMID)"
export MAS_PROVISION="/path/to/scrannotate.provisionprofile"

cargo build --release --locked --target aarch64-apple-darwin
packaging/macos/mas-package.sh \
  target/aarch64-apple-darwin/release/scrannotate 0.4.0 dist/scrannotate-mas.pkg

# Upload (or drag the .pkg into Transporter.app):
xcrun altool --upload-app -f dist/scrannotate-mas.pkg -t macos \
  --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
```

Then in App Store Connect: attach the build to a version, fill in metadata
and screenshots, and submit for review. The Mac App Store accepts a
**universal** binary — build both arches and `lipo` them (mirror
`build.sh --universal`) so one package covers Apple silicon and Intel.

---

## Microsoft Store (MSIX)

`.msix` package uploaded to Partner Center. The Store **re-signs** your
package on upload, so you do **not** need to buy a code-signing certificate
for Store distribution (you only need one to sideload/test locally).

### What you need

1. A **Microsoft Partner Center** account with the **Windows & Xbox**
   (app developer) program — <https://partner.microsoft.com>. One-time
   registration fee.
2. **Reserve the app name** in Partner Center. From the reservation, copy the
   three identity values (Product identity → "App management"):
   - **Package/Identity/Name** → `MSIX_IDENTITY_NAME`
   - **Package/Identity/Publisher** (the `CN=...` string) → `MSIX_PUBLISHER`
   - **Publisher display name** → `MSIX_PUBLISHER_DISPLAY_NAME`

   These must match the manifest **exactly** or upload is rejected.
3. The tile/logo assets in `packaging/windows/Assets/` (see its README).
4. **Windows SDK** on the build machine (`makeappx.exe`; `signtool.exe` only
   for local sideload signing).

### Restricted capabilities

`AppxManifest.xml` declares `graphicsCaptureProgrammatic` and
`graphicsCaptureWithoutBorder` — **restricted** capabilities that require a
written justification during submission and can lengthen review. If your
build captures fine without them (plain Windows Graphics Capture with the
system's capture picker/border), delete those two lines to smooth review.

### Build & upload

```powershell
# Store package (unsigned — the Store signs it on upload):
$env:MSIX_IDENTITY_NAME          = '12345AppCove.scrannotate'
$env:MSIX_PUBLISHER              = 'CN=ABCD1234-...'   # exact string from Partner Center
$env:MSIX_PUBLISHER_DISPLAY_NAME = 'AppCove, Inc.'

cargo build --release --locked
packaging\windows\build-msix.ps1 -Exe target\release\scrannotate.exe `
  -Version 0.4.0.0 -Arch x64      # four-part version; Store requires revision 0
```

Upload the `.msix` (build both `x64` and `arm64` and submit them together)
under your app's **Packages** in Partner Center, complete the Store listing,
and submit.

To **sideload-test** before submitting, sign with a cert whose Subject equals
`MSIX_PUBLISHER`:

```powershell
packaging\windows\build-msix.ps1 -Exe target\release\scrannotate.exe `
  -Version 0.4.0.0 -Arch x64 -SignCert scrannotate-test.pfx `
  -SignPassword (Read-Host -AsSecureString)
# then: Add-AppxPackage dist\scrannotate-x64.msix   (trust the cert first)
```

---

## CI secrets

`.github/workflows/store-release.yml` is **manual** (`workflow_dispatch`) and
each job runs only when its secrets are present, so the workflow is inert
until you add them. Set these under **Settings → Secrets and variables →
Actions**:

### macOS (Mac App Store)

| Secret | What it is |
|--------|------------|
| `MAS_CERT_P12` | base64 of a `.p12` bundling **both** 3rd Party Mac Developer certs + keys |
| `MAS_CERT_PASSWORD` | password for that `.p12` |
| `MAS_PROVISION_BASE64` | base64 of the `.provisionprofile` |
| `MAS_APP_CERT` | the Application cert's full name (see above) |
| `MAS_INSTALLER_CERT` | the Installer cert's full name |
| `ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_API_KEY_P8` | App Store Connect API key (id, issuer, base64 of the `.p8`) |

### Windows (Microsoft Store)

| Secret | What it is |
|--------|------------|
| `MSIX_IDENTITY_NAME` | reserved Package Identity Name |
| `MSIX_PUBLISHER` | reserved Publisher `CN=...` |
| `MSIX_PUBLISHER_DISPLAY_NAME` | Publisher display name |
| `PARTNER_CENTER_TENANT_ID`, `PARTNER_CENTER_CLIENT_ID`, `PARTNER_CENTER_CLIENT_SECRET` | Azure AD app for the Store Submission API (optional — only for automated upload; otherwise upload the built artifact by hand) |

> Store **identity** values (`MSIX_*`) are not secret, but keeping them in
> Actions secrets lets the same workflow serve a private fork without editing
> the manifest. Move them to repository **variables** if you prefer.
