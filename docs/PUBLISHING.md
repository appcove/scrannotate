# Publishing runbook — step by step

The exact, ordered steps to get scrannotate onto the **Mac App Store** and
the **Microsoft Store**: what to click in each portal, what to build, and the
commands to run — in sequence, first submission to last.

- Reference for certs/secrets/identity values: [SIGNING.md](SIGNING.md).
- The scripts this runbook calls live in `packaging/`.
- Do the **one-time prerequisite** first; then each store section is
  self-contained and can be done in either order.

---

## One-time prerequisite: the app icon

Both stores reject an app with no icon, and scrannotate ships none. Do this
once before either submission.

1. Design a **1024×1024 PNG** master (opaque, no rounded corners — the OSes
   round them).
2. **macOS `.icns`:**
   ```bash
   mkdir scrannotate.iconset
   # Produce the required sizes from your 1024 master (sips is built in):
   for s in 16 32 128 256 512; do
     sips -z $s $s   master_1024.png --out scrannotate.iconset/icon_${s}x${s}.png
     sips -z $((s*2)) $((s*2)) master_1024.png --out scrannotate.iconset/icon_${s}x${s}@2x.png
   done
   cp master_1024.png scrannotate.iconset/icon_512x512@2x.png
   iconutil -c icns scrannotate.iconset -o packaging/macos/AppIcon.icns
   ```
3. **Windows tiles:** create the PNGs listed in
   `packaging/windows/Assets/README.md` (sizes 44/150/310×150/50) from the
   same master and drop them in `packaging/windows/Assets/`. Visual Studio's
   Manifest Designer or any icon-asset generator makes the full scaled set.

---

## A. Mac App Store

You already have an Apple Developer account, so start at the developer portal.

### A1. Register the App ID  (developer.apple.com)

1. Go to **Certificates, Identifiers & Profiles → Identifiers → +**.
2. Choose **App IDs → App**.
3. **Description:** `scrannotate`. **Bundle ID:** *Explicit* →
   `com.appcove.scrannotate` (must match `packaging/macos/Info.plist`).
4. Leave Capabilities unchecked (App Sandbox is an entitlement, set by our
   build — not a portal capability; screen capture needs no App ID
   capability). **Continue → Register.**

### A2. Create the two distribution certificates

You need a Certificate Signing Request first:

1. **Keychain Access → Certificate Assistant → Request a Certificate From a
   Certificate Authority.** Enter your email, leave "Saved to disk", save
   `CertificateSigningRequest.certSigningRequest`.
2. In the portal, **Certificates → +**, create each of these (uploading the
   same CSR both times), then **download** and **double-click** each to
   install into your login keychain:
   - **Mac App Distribution** → installs as *3rd Party Mac Developer
     Application: … (TEAMID)*
   - **Mac Installer Distribution** → installs as *3rd Party Mac Developer
     Installer: … (TEAMID)*
3. Confirm they landed:
   ```bash
   security find-identity -v | grep "3rd Party Mac Developer"
   ```
   Copy the two full names — they become `MAS_APP_CERT` and
   `MAS_INSTALLER_CERT`.

### A3. Create the provisioning profile

1. **Profiles → + → Mac App Store (Distribution) → Continue.**
2. Select the `com.appcove.scrannotate` App ID → select the **Mac App
   Distribution** certificate → name it `scrannotate MAS` → **Generate**.
3. **Download** the `.provisionprofile`; note its path for `MAS_PROVISION`.

### A4. Create the app record  (App Store Connect)

1. <https://appstoreconnect.apple.com> → **Apps → + → New App.**
2. **Platform:** macOS. **Name:** scrannotate (must be unique across the
   Store — pick a fallback if taken). **Primary language.** **Bundle ID:**
   select `com.appcove.scrannotate`. **SKU:** any internal string, e.g.
   `scrannotate-mac`. **Full/Limited access** as you like → **Create.**

### A5. Create an API key for uploading

1. **Users and Access → Integrations → App Store Connect API → Team Keys →
   Generate API Key.** Role **App Manager** is enough.
2. Note the **Key ID** and **Issuer ID**; **download the `.p8`** (one-time
   download). These are `ASC_KEY_ID`, `ASC_ISSUER_ID`, and the key file.

### A6. Build, package, upload

Make sure the icon from the prerequisite exists at
`packaging/macos/AppIcon.icns`, then:

```bash
# Names/paths from A2 and A3:
export MAS_APP_CERT="3rd Party Mac Developer Application: AppCove, Inc. (TEAMID)"
export MAS_INSTALLER_CERT="3rd Party Mac Developer Installer: AppCove, Inc. (TEAMID)"
export MAS_PROVISION="/absolute/path/to/scrannotate.provisionprofile"

# Universal binary (covers Apple silicon + Intel in one package):
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin
lipo -create -output /tmp/scrannotate \
  target/aarch64-apple-darwin/release/scrannotate \
  target/x86_64-apple-darwin/release/scrannotate

# Sandbox-sign the .app + build & sign the installer .pkg:
packaging/macos/mas-package.sh /tmp/scrannotate 0.4.0 dist/scrannotate-mas.pkg
```

Upload the `.pkg` to App Store Connect one of two ways:

```bash
# Option 1 — command line (put the .p8 where altool looks for it):
mkdir -p ~/.appstoreconnect/private_keys
cp /path/to/AuthKey_<KEYID>.p8 ~/.appstoreconnect/private_keys/
xcrun altool --upload-app -f dist/scrannotate-mas.pkg -t macos \
  --apiKey <KEYID> --apiIssuer <ISSUER-ID>
```
```
# Option 2 — GUI: open Transporter.app (free, Mac App Store), sign in,
# drag in dist/scrannotate-mas.pkg, Deliver.
```

The build takes a few minutes to finish processing in App Store Connect.

### A7. Fill the listing and submit  (App Store Connect)

In your app → the version (e.g. **1.0**) under **macOS App**:

1. **Screenshots:** at least one, sized **1280×800**, **1440×900**,
   **2560×1600**, or **2880×1800** (capture scrannotate itself with ⌘⇧5).
2. **Promotional text / Description / Keywords / Support URL / Marketing
   URL** (support URL is required — the GitHub repo works).
3. **Build:** click **+ / Select a build** and pick the one from A6 (it
   appears once processing completes).
4. **General → App Information:** Category (e.g. *Photo & Video* or
   *Productivity*), **Content Rights**, **Age Rating** questionnaire.
5. **App Privacy:** answer the data-collection questions — scrannotate
   collects nothing and needs no network, so answer *No data collected* (also
   set a **Privacy Policy URL**; a short page stating "no data collected"
   suffices).
6. **Pricing and Availability:** set price (Free) and territories.
7. **Review notes:** explain the screen-recording prompt — *"The app uses
   ScreenCaptureKit to take the screenshot the user annotates; macOS shows
   the standard Screen Recording permission prompt on first capture."* This
   pre-empts the most likely review question.
8. **Add for Review → Submit.**

If review rejects the silent save to `~/Pictures`, switch Ctrl+S to an
`NSSavePanel` and change the entitlement (see SIGNING.md → sandbox caveats),
then re-build and re-upload with a bumped version.

---

## B. Microsoft Store (MSIX)

### B1. Register and reserve the app  (partner.microsoft.com)

1. Sign in to **Partner Center**; if you haven't, enroll in the
   **Windows & Xbox** (app developer) program (one-time fee).
2. **Apps and games → New product → App.**
3. **Reserve the app name** `scrannotate` (pick a fallback if taken).
4. Open the product → **Product management → Product identity.** Copy the
   three values — you'll paste them into the build:
   - **Package/Identity/Name** → `MSIX_IDENTITY_NAME`
   - **Package/Identity/Publisher** (the `CN=…` string) → `MSIX_PUBLISHER`
   - **Publisher display name** → `MSIX_PUBLISHER_DISPLAY_NAME`

### B2. Build the MSIX packages

On a Windows machine with the **Windows SDK** installed, with the tile assets
from the prerequisite in `packaging/windows/Assets/`:

```powershell
$env:MSIX_IDENTITY_NAME          = '12345AppCove.scrannotate'   # from B1
$env:MSIX_PUBLISHER              = 'CN=ABCD1234-....'           # exact string from B1
$env:MSIX_PUBLISHER_DISPLAY_NAME = 'AppCove, Inc.'             # from B1

# x64:
cargo build --release --locked
packaging\windows\build-msix.ps1 -Exe target\release\scrannotate.exe -Version 0.4.0.0 -Arch x64

# arm64 (build on a windows-11-arm machine, or cross-target):
packaging\windows\build-msix.ps1 -Exe target\release\scrannotate.exe -Version 0.4.0.0 -Arch arm64
```

Leave the packages **unsigned** — the Store re-signs on upload. (To smoke-test
locally first, pass `-SignCert` per SIGNING.md and `Add-AppxPackage` it.)

Version note: the manifest needs a **four-part** version and the last part
must be **0** (`0.4.0.0`), per Store rules.

### B3. Create the submission  (Partner Center)

In the product, start a submission and fill each section:

1. **Pricing and availability:** Free, markets, release schedule.
2. **Properties:** Category (e.g. *Photo & video* or *Productivity*),
   privacy policy URL (state "no data collected"), support contact.
3. **Age ratings:** complete the questionnaire (generates IARC ratings).
4. **Packages:** upload the `.msix` files from B2 (both x64 and arm64).
   If the manifest keeps the **restricted** capabilities
   (`graphicsCaptureProgrammatic`, `graphicsCaptureWithoutBorder`), a
   justification box appears — explain: *"The app captures the screen with
   Windows Graphics Capture to produce the screenshot the user annotates."*
   (Drop those two lines from `AppxManifest.xml` if your build doesn't need
   them — it smooths review.)
5. **Store listing:** description, at least one **screenshot** (min
   1366×768), the logos, and a short "what's new".
6. **Submit to the Store.**

---

## C. Doing it from CI instead

Once the certs/identity values are set as repository secrets (tables in
[SIGNING.md](SIGNING.md)), you can skip the local build/package/upload steps
(A6 and B2): go to the repo's **Actions → Store release → Run workflow**. The
macOS job builds, packages, and uploads to App Store Connect; the Windows job
builds the MSIX and leaves it as a downloadable artifact for you to upload in
B3. You still do the portal setup (A1–A5, B1) and the listing/submit steps
(A7, B3) by hand — those are one-time and can't be scripted.
