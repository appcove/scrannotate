# Windows and Mac App Store readiness audit

**Assessment: do not submit the current build to either store.** The editor has a useful test suite and an existing cross-platform build pipeline, but store packaging, macOS sandbox integration, privacy presentation, and several application behaviors need work first.

Audited on **2026-09-20**, at commit **2031230b7988414c534bbaa09e613a388af13e2f**, package version **0.4.0**. Scope includes tracked application code, locked dependencies, packaging, workflows, documentation, public release archive contents, and current primary-source store documentation. This report is the only repository change made by the audit.

**Evidence labels:** “reproduced” means application code was exercised on this Linux host; “confirmed in source” means the implementation or missing configuration was inspected; “native validation required” means the predicted platform behavior still needs Windows/macOS execution. A submission **blocker** prevents a credible store submission; **high** findings should be fixed before public release; **medium** findings need resolution or a deliberate, documented limitation. These priorities describe release readiness, not CVSS scores.

## Verification performed

| Check | Result and limits |
| --- | --- |
| `cargo test --locked --no-default-features` | **43 passed**, zero failures. Tests cover document transactions, editor interactions, geometry, view transforms, text-copy decisions, preference color conversion, and basic export. Capture was disabled. |
| `cargo clippy --all-targets --no-default-features --locked -- -D warnings` | **Passed** with Rust 1.96.1. |
| `cargo test --locked` | **Environment-blocked**: `libspa-sys` could not find the host's `libpipewire-0.3` development package. This is not evidence of an application compile failure on supported CI hosts. |
| `cargo fmt --all -- --check` | **Failed**: broad formatting differences with the installed rustfmt; no formatting changes applied. |
| `bash -n packaging/macos/bundle.sh` | **Passed**; does not validate an Apple signature or package. |
| `cargo metadata --locked --format-version 1` and target-specific `cargo tree` | Inspected 495 resolved packages across platforms; identified actual advisory dependency paths and minimum Rust version mismatch. |
| `cargo-audit 0.22.2 audit --json` | **Failed**: three vulnerability advisories and two informational warnings. Exposure assessment appears below. Database commit `d5c17953a895cf19e8d3ce66eaa42b6fcfe1fb16`, updated September 19, 2026. |
| Temporary harness importing the actual project modules | Reproduced filename overwrite, toolbar overflow, transparent-image export corruption, and emoji export mismatch. No project source was modified. |
| Published v0.4.0 archive inspection | Windows x64 ZIP contains only `scrannotate.exe`. Apple Silicon app ZIP contains the executable, `Info.plist`, and code-signature resources; no application resources or license documents. These are existing public artifacts, not builds of the audited local commit. |
| Windows/macOS installation and execution | **Not performed**: this host is Linux; no native machines, store accounts, signing identities, or submission access were available. Existing CI configuration is not proof of signed-package runtime behavior. |

## Submission blockers

### S1. macOS release output is not a Mac App Store submission package

**Blocker · confirmed in source and existing archive.**

Evidence: [bundle.sh](../packaging/macos/bundle.sh), lines 29–41; [release.yml](../.github/workflows/release.yml), lines 124–144; [Info.plist](../packaging/macos/Info.plist).

The script copies a binary and plist, signs with `codesign --sign -`, and produces `.app.zip`. There is no store distribution signing configuration, App Sandbox entitlement, provisioning integration, signed installer package, or submission validation. The plist also lacks `LSApplicationCategoryType` and app icon configuration; the bundle has no icon resources.

Create a distinct Mac App Store packaging path with the registered bundle identity, appropriate application/installer distribution identities and provisioning, sandbox entitlements, complete icon assets, category, and copyright metadata. Build a signed package using Apple's packaging tools, inspect the resulting signature and entitlements, and validate through the supported submission tooling. Use a universal executable if retaining both Intel and Apple Silicon support in one app is the product decision.

Apple documents [App Sandbox](https://developer.apple.com/documentation/security/app-sandbox), [Mac software packaging](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution), and the [required App Store category](https://developer.apple.com/library/archive/documentation/General/Conceptual/MOSXAppProgrammingGuide/BuildTimeConfiguration/BuildTimeConfiguration.html). Developer ID signing and notarization concern the separate direct-download route; notarizing the existing ZIP alone would not resolve the store requirements.

**Acceptance:** a production-equivalent signed, sandboxed package validates, installs, launches, captures, saves, copies, and updates through the intended store testing path.

### S2. Windows has no supported Store packaging route implemented

**Blocker · confirmed in source and existing archive.**

Evidence: [release.yml](../.github/workflows/release.yml), lines 134–144; [ci.yml](../.github/workflows/ci.yml), lines 61–62. There is no Windows packaging directory, installer definition, MSIX manifest, package logo set, or package validation step.

The existing ZIP containing a portable executable is not a submission-ready installer or MSIX package. Choose one route:

| Route | Work needed |
| --- | --- |
| **MSIX — recommended for this app** | Package the existing Win32 executable with Partner Center identity, desktop entry point, architecture, compatible minimum OS, version mapping, visual assets, and appropriate full-trust declaration. Produce x64/ARM64 packages or a bundle. Register an execution alias if preserving the documented CLI workflow. |
| EXE/MSI | Build a standalone offline installer with silent installation, uninstall/installed-program integration, and an immutable versioned HTTPS download URL. Authenticode-sign the installer and every included PE file with a certificate chaining to a CA in the Microsoft Trusted Root Program. |

Store-distributed MSIX packages are signed by Microsoft; buying a CA-backed signing certificate is not required for that route. EXE/MSI submission has different signing responsibilities. See Microsoft's [MSIX requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements), [EXE/MSI requirements](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msi/app-package-requirements), and [desktop package manifest guidance](https://learn.microsoft.com/en-us/windows/msix/desktop/desktop-to-uwp-manual-conversion).

Do not mechanically copy Cargo's `0.4.0` into a store manifest. Define and validate a store-compatible numeric version scheme, with monotonically increasing updates and matching versions across architectures.

**Acceptance:** the selected package passes the applicable validation, installs under a standard user, launches from Start/Store, upgrades without breaking preferences, and uninstalls cleanly. Run WACK for the MSIX route.

### S3. macOS sandbox file access requires application changes

**Blocker for the sandboxed build · confirmed missing implementation; exact default-directory behavior needs native validation.**

Evidence: [main.rs](../src/main.rs), lines 49–63 and 133–157; [export.rs](../src/export.rs), lines 21–26; [prefs.rs](../src/prefs.rs), lines 26–31.

The app accepts arbitrary `--from-file` and `--save-path` values and directly reads or writes them. There is no native Open/Save panel, folder authorization flow, or security-scoped bookmark handling. A CLI path string does not itself authorize access outside an App Sandbox container. Enabling sandboxing alone can therefore break image import/custom destinations. The home-derived default may also place files inside the container rather than the user's expected Pictures directory.

Add authorized native file/folder selection and the corresponding [user-selected read/write entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.files.user-selected.read-write). Persist security-scoped access if remembering an external folder between launches. Deliberately choose the default save destination and verify preference storage and any migration from the existing unsandboxed app. Screen Recording consent remains a separate concern from filesystem authorization.

**Acceptance:** exercise default saving, chosen external folders, reopened bookmarks, denied access, moved/deleted folders, and file import using the signed sandboxed app.

### S4. Privacy policy presentation is missing

**Blocker · in-app absence confirmed; externally hosted policies and account metadata were not inspected.**

Evidence: the complete UI in [toolbar.rs](../src/ui/toolbar.rs), [app.rs](../src/app.rs), and [main.rs](../src/main.rs) has no privacy link, About/Help location, or support link. No privacy document or configured URL is tracked.

Apple requires a privacy policy link in the app and submission metadata under [guideline 5.1.1(i)](https://developer.apple.com/app-store/review/guidelines/). Microsoft explicitly requires privacy policies for Win32/Desktop Bridge products in [policy 7.19, section 10.5.1](https://learn.microsoft.com/en-us/windows/apps/publish/store-policy-archive/store-policy-7-19#105-personal-information), even when processing is local. Version 7.19 is effective on this audit date; the published successor, 7.20, takes effect October 22, 2026 and retains this requirement.

Publish a policy and add an accessible in-app entry. Describe screen/image access, transient editing, user-requested PNG writes, clipboard transfer, local preferences, consent revocation, deletion, support contact, and actual telemetry practices. Clipboard contents and chosen save folders can be synchronized by the operating system or another service; avoid an unconditional claim that data can never leave the device.

No application-owned upload, analytics, account, advertising, or network client flow was found in `src`. That supports an initial local-processing assessment, not a dynamic proof about the final binary. Apple's definition of collected data distinguishes off-device collection from local processing; assess the final app and SDKs before choosing the [App Privacy answers](https://developer.apple.com/app-store/app-privacy-details/).

**Acceptance:** live policy/support URLs, in-app access, matching store declarations, and review notes explaining screen capture and local storage.

## Application findings

### F1. Startup and permission failures can exit without a visible explanation

**High · confirmed control flow; native permission timing requires testing.**

Evidence: [main.rs](../src/main.rs), lines 4, 113–117, 133–156, and 198–206; [capture/session.rs](../src/capture/session.rs), line 43.

Capture and image loading happen before the GUI exists and propagate failures out of `main`. On Windows the GUI subsystem has no console for ordinary Store/Explorer launches; on macOS Finder launches do not expose the stderr error. Screen permission denial/revocation, a disconnected monitor, a bad image, or renderer initialization failure can therefore appear as “nothing happened.” The frame wait permits up to 120 seconds without an application progress window.

Provide a native startup error dialog or an application startup state with explanation, retry/cancel, permission guidance, and a file-open alternative. Preserve the detailed error for diagnostics without logging screen contents. Test first permission request, denial, later grant, revocation, interrupted capture, and unavailable renderer.

### F2. Repeated saves silently overwrite a prior screenshot

**High · reproduced.**

Evidence: [export.rs](../src/export.rs), lines 21–26.

Output names have whole-second precision, and `img.save` overwrites an existing file. Two toolbar saves or concurrent instances in the same second can return success for the same destination. The reproduction saved a red image followed by a green image: both paths were equal and the first saved image became green.

Use exclusive file creation and retry with a suffix or unique identifier. Higher timestamp precision alone does not guarantee no collisions. Prefer a write strategy that does not leave a completed-looking partial output after encoding or disk failure.

**Acceptance:** repeated and concurrent saves preserve every image; collision and write-failure tests cover the actual filesystem behavior.

### F3. macOS 12.3 support conflicts with the locked capture implementation

**High · confirmed unavailable API call; native crash reproduction pending.**

Evidence: [Info.plist](../packaging/macos/Info.plist), lines 23–24; [README.md](../README.md), line 141; [Cargo.lock](../Cargo.lock), line 2880. The locked `pinray-platform-macos 0.2.4`, `src/capture/config.rs:115`, unconditionally calls `setCapturesAudio(capture_audio)`, including when audio is disabled.

Apple introduced [`SCStreamConfiguration.capturesAudio`](https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/capturesaudio) in macOS 13.0, as recorded in its [API availability metadata](https://developer.apple.com/tutorials/data/documentation/screencapturekit/scstreamconfiguration/capturesaudio.json). Calling the missing selector on 12.x is expected to fail despite the plist advertising 12.3.

Patch/upgrade the backend to omit or availability-guard the setter when appropriate, or raise the minimum OS. Align the actual Mach-O deployment target, plist, store metadata, and documentation; validate every remaining API on the declared minimum OS.

### F4. macOS display scale and primary-display metadata are inaccurate

**Medium · confirmed in the dependency; visual/runtime results need native validation.**

Evidence: [Cargo.lock](../Cargo.lock), line 2880; `pinray-platform-macos 0.2.4/src/capture/config.rs:89–95,158–162` and `src/content.rs:65–72`; [capture/monitor.rs](../src/capture/monitor.rs), lines 25–38.

The backend always uses a scale factor of 2 when requesting capture dimensions. Apple describes [`SCDisplay` dimensions in points](https://developer.apple.com/documentation/screencapturekit/scdisplay). A 1× 1920×1080 display therefore requests 3840×2160 output: incorrect native-resolution output and four times the pixels. Scaled Retina modes also need explicit validation.

Separately, every Mac display is marked `is_primary: false`, making the app's primary-first sort ineffective and preventing its primary marker from appearing. The README/CLI promise of primary-first numbering is unsupported by this metadata.

Use actual display mode/point-to-pixel information and primary-display identity. Test external 1×, Retina, scaled modes, reassigned primary display, docking, and mixed displays. A dependency upgrade only resolves these findings if its implementation has actually changed.

### F5. Windows backend behavior disagrees with the documentation and misses rotation/cursor handling

**High for rotation; medium for cursor/documentation · implementation confirmed, native reproduction required.**

Evidence: [capture/session.rs](../src/capture/session.rs), lines 15–19; [Cargo.lock](../Cargo.lock), line 2898; [README.md](../README.md), lines 152–157. Locked `pinray-platform-windows 0.2.4/src/lib.rs:104–112` selects **DXGI first, WGC on initialization failure**, opposite to the README's WGC-first description.

The DXGI path passes raw acquired textures through `src/dxgi.rs:159` and `src/d3d.rs:85–97,161–180` without correcting desktop rotation. It also does not composite separate pointer metadata or honor the requested embedded cursor. Microsoft documents both the need for rotation correction and the possibility of a separate hardware cursor in the [Desktop Duplication API](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api). Portrait/flipped captures are consequently at risk of incorrect orientation, and `--cursor` can omit the pointer on the normal backend.

Patch these behaviors or deliberately select a verified WGC implementation, including failure/fallback handling. Test 90°, 180°, and 270° displays, cursor on/off, mixed GPUs, and actual exported pixels. Also qualify the Windows 11 border-suppression claim: setting `IsBorderRequired(false)` alone is not proof of authorization for a packaged application; Microsoft documents the [consent/capability flow](https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired?view=winrt-26100).

### F6. The toolbar extends below common scaled desktops

**High usability impact · reproduced using actual egui layout.**

Evidence: [toolbar.rs](../src/ui/toolbar.rs), lines 78–86 and 274–326.

On a 960×540 logical canvas, equivalent to 1920×1080 at 200% scaling, the toolbar measured **372×755 logical pixels**, with bounds `(12, 0)` to `(384, 755)`. Placement clamps its top to the canvas but supplies no scrolling or adaptive arrangement. Lower Copy/Save/Close controls and status messages can fall below the visible area, and moving the toolbar cannot bring its bottom into view.

Constrain its height to available space and provide scrolling or an adaptive layout that keeps primary actions and errors reachable. Test 720p/1080p displays at 100–200% scaling, larger accessibility text, and small Mac display modes.

### F7. Transparent PNG imports export incorrect colors

**Medium · reproduced.**

Evidence: [main.rs](../src/main.rs), lines 140–144; [export.rs](../src/export.rs), lines 40–52.

The input path accepts transparent PNGs, but export assumes the base is opaque. Straight RGBA is passed into a renderer that uses premultiplied RGBA, and the result is returned without unpremultiplication. A red highlight over transparent pixels exported `[70, 0, 0, 70]` instead of approximately `[255, 0, 0, 70]`, causing darkened output when composited elsewhere.

Convert alpha representation at both boundaries or intentionally flatten imported images against a documented background. Cover partially transparent base images, edges, text, rotated text, PNG output, and clipboard output with pixel assertions.

### F8. Export does not share the UI's font fallback

**Medium · reproduced.**

Evidence: [export.rs](../src/export.rs), lines 44–45 and 318–326; [paint.rs](../src/ui/paint.rs), lines 89–99; [text_overlay.rs](../src/ui/text_overlay.rs), lines 55–69.

The UI uses egui's proportional font fallback chain, including Noto Emoji; export resolves every character against Ubuntu Light only. `😀` and `🚀` both became glyph zero in export, and the two exported images were byte-identical despite distinct available UI fallback glyphs. The README's pixel-identical export claim is therefore too strong.

Share font selection and layout across preview/export, or clearly constrain the supported text repertoire. Add representative fallback characters and multiline/rotated text comparisons; verify IME/composed input on both platforms before claiming broad text support.

### F9. Save and Quit shortcuts are disabled while editing text

**Medium · confirmed in source.**

Evidence: [app.rs](../src/app.rs), lines 294–298 and 307–321; [text_overlay.rs](../src/ui/text_overlay.rs), lines 32–119; [main.rs](../src/main.rs), lines 188–195.

The global shortcut handler returns immediately during inline text editing. The text widget handles Copy, Enter, and Escape but not application Save or Quit. On macOS this also disables Cmd+Q/Cmd+W because the native menu has been removed specifically to route those commands through egui.

Dispatch lifecycle commands before the text-only early return, committing text where appropriate. Test Save/Quit/Close while typing and while widgets have focus. Format visible shortcuts using platform-aware modifiers: toolbar/status strings currently say Ctrl on macOS even though the application uses Command.

### F10. Feedback, destinations, and advanced options need a Store launch path

**Medium · confirmed code paths; sandbox/redirected-folder cases need native validation.**

Evidence: [toolbar.rs](../src/ui/toolbar.rs), lines 54–64; [app.rs](../src/app.rs), lines 207–250; [main.rs](../src/main.rs), lines 29–63.

Three related gaps affect users launching from Finder/Start:

- Save/Copy can export the whole frame before a region exists, but failures are displayed only in the toolbar, which returns early when there is no region. The five-second error message can remain entirely invisible.
- Save-and-close only prints the destination to stdout before exiting. Windows output location guesses `home/Pictures`, ignoring redirected Known Folders such as OneDrive Pictures; its fallback is `home/Screenshots`.
- Monitor selection, cursor, delay, output folder, and image import are CLI-only. There is no built-in GUI chooser/settings/help path exposing these options. Under MSIX, documented terminal use also needs an execution alias or equivalent integration.

Render errors independently of the selection toolbar, resolve native user directories, and make save destinations discoverable. Add a compact settings/help or launch interface appropriate to the intended Store audience, with file/folder panels also satisfying S3. An integrated global hotkey is a product choice, not an automatic store requirement.

**Additional interaction edge case:** export shortcuts remain active during unfinished drawing gestures, while `rendered()` only reads committed document shapes. Pressing Enter during an active Blur/shape drag can omit that unfinished annotation and close the app. Block export during incomplete gestures or explicitly finalize them; include this in interaction tests.

## Release engineering and distribution

### R1. Publication does not depend on successful CI

**High · confirmed in workflow configuration.**

Evidence: [release.yml](../.github/workflows/release.yml), lines 32–38 and 155–164; [ci.yml](../.github/workflows/ci.yml), lines 9–12 and 41–45.

Release and CI run independently on a qualifying push. The publish job waits for release asset builds, but not for Clippy/tests in the separate CI workflow. A buildable commit with failing tests can therefore publish. Branch protections, if configured externally, were not inspected and do not make this workflow itself dependent on post-merge checks.

Gate packaging/publication on required checks for the exact source commit, preferably using a shared validation job or promoting an already-tested artifact. Add signed-package installation/validation to the store release gate. Preserve a clear separation between building reviewable artifacts and submitting/publishing them.

### R2. Resuming a draft can associate new code with an older release target

**High · confirmed source-provenance flaw; no live release was modified.**

Evidence: [release.yml](../.github/workflows/release.yml), lines 76–85, 88–93, 119, 127–138, and 150–153.

Preparation resumes any existing draft for the same version. Asset jobs check out the triggering revision rather than resolving and checking out the original release target. A later manual dispatch or workflow change can therefore build commit B for a draft originally targeted at commit A, without changing the version or proving agreement. The upload action's `ref` selects the destination release; it does not replace the working checkout. Existing app ZIPs can also be overwritten with `--clobber`.

Resolve one immutable release SHA, use it in every build, verify the version/tag/target relationship, and fail if an existing draft targets different source. Associate checksums, package versions, symbols, and any attestations with that SHA. Exercise a failed-build/resume scenario in a nonpublic test release flow.

### R3. Binary packages omit license and attribution documents

**High distribution gap · confirmed packaging omission; a complete license compliance assessment remains to be done.**

Evidence: [bundle.sh](../packaging/macos/bundle.sh), lines 29–41; [release.yml](../.github/workflows/release.yml), lines 134–144; [NOTICE](../NOTICE); [Cargo.toml](../Cargo.toml), lines 23–32. Public archive inspection corroborated the omission.

Neither inspected archive contains this project's LICENSE/NOTICE or third-party notices. The Mac bundler copies no license resources; the binary upload action has no additional-file inclusion setting. The small project NOTICE is not a dependency attribution inventory, and the GUI exposes no acknowledgments.

Embedded fonts require particular attention: `epaint_default_fonts 0.35.0` declares `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` and includes additional font notices. The [Ubuntu Font Licence](https://github.com/emilk/egui/blob/0.35.0/crates/epaint_default_fonts/fonts/UFL.txt) and [SIL Open Font License](https://openfontlicense.org/open-font-license-official-text/) contain redistribution notice requirements. An Apache-2.0 label on this project does not replace dependency terms.

Generate a target-aware dependency/license inventory, include the applicable texts and copyright notices in every store package, and expose them through an acknowledgments view or accessible resource. Review MPL and dual-license choices based on the dependencies actually shipped, rather than treating every platform's lockfile package as bundled. Verify the packaged result, not just the repository files.

### R4. Build reproducibility and documented prerequisites need alignment

**Medium · confirmed.**

Evidence: [Cargo.toml](../Cargo.toml), lines 1–4 and 28–29; [README.md](../README.md), source build prerequisites; [CONTRIBUTING.md](../CONTRIBUTING.md), development prerequisites/release description; both workflows' `rust-toolchain@stable` steps.

The documentation says Rust 1.88+, but the locked egui/eframe 0.35 packages require **Rust 1.92**. The package has no `rust-version`, and no toolchain file pins release tooling. Rolling stable and mutable action tags can change release inputs independently of source. Clippy passes under the audited toolchain, but rustfmt check fails broadly and is not in CI. CONTRIBUTING also describes tag-triggered releases while the actual workflow triggers on selected main-branch pushes or manual dispatch.

Declare the real minimum Rust version, pin/record the production toolchain and native SDKs, choose an intentional formatting configuration, and correct contributor documentation. Pin third-party release actions to reviewed commits before giving them signing/submission credentials. Retain symbols and build provenance for diagnosing store crashes; avoid silently rebuilding previously validated artifacts with different tools.

## Dependency advisory assessment

**Medium release hygiene issue; no reachable Windows/macOS exploit was established by this scan.** A lockfile advisory is not automatically a vulnerability reachable through this app's UI.

| Locked dependency | Advisory and fix | Assessed relevance |
| --- | --- | --- |
| `quick-xml 0.39.4` | [RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194.html): excessive attribute-processing time. Fixed in 0.41.0+. | Reached through `wayland-scanner`, a Linux build-time procedural macro processing protocol XML. Not part of the Windows/macOS runtime dependency path. |
| `quick-xml 0.39.4` | [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195.html): namespace allocation exhaustion. Fixed in 0.41.0+. | Same build dependency; the inspected scanner uses plain `Reader`, not the affected `NsReader` path. No app-level XML import was found. |
| `webbrowser 1.2.1` | [RUSTSEC-2026-0257](https://rustsec.org/advisories/RUSTSEC-2026-0257.html): argument injection through Unix `BROWSER` handling. Fixed in 1.2.2+. | Transitive through egui-winit. Its affected Unix module excludes macOS and Windows; no application URL-opening flow was found. Still update the lockfile for the Linux product and future link features. |
| `event-listener 5.4.1` | [RUSTSEC-2026-0221](https://rustsec.org/advisories/RUSTSEC-2026-0221.html): informational unsoundness warning; fixed in 5.4.2+. | Linux dependency paths through zbus/accessibility/capture. No affected custom-tag usage was demonstrated. |
| `ttf-parser 0.25.1` | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html): unmaintained; no patched version. | Included through ab_glyph in native export. The app uses embedded fonts rather than accepting arbitrary font files. Track replacement/upstream maintenance; this warning alone does not establish a security exploit. |

Update compatible dependencies, handle the quick-xml major-minor change through its parent dependency rather than forcing incompatible constraints, and rerun platform checks. Add scheduled advisory checks and license checks to CI. If temporarily accepting an advisory, record affected platform/code path, rationale, owner, and review date; avoid permanent blanket suppression.

## Privacy and security observations

- Screenshot processing is local in the reviewed application source. Capture frames are edited in memory; output is written or copied at the user's request. Preferences persist colors and sizes, not screenshot pixels. No credentials, service backend, account system, updater, or application telemetry flow was found in the tracked application code.
- Preserve this small data footprint when adding diagnostics. Do not automatically attach screenshots, annotation text, clipboard content, or private file paths to crash reports.
- Pixelation is a visual obfuscation feature, not a demonstrated irreversible secret-redaction guarantee. The original frame also remains in the in-memory document while editing. Avoid security claims about Blur without a dedicated threat model and tests; consider an opaque redaction tool if secure removal is a product requirement.
- The capture backends and renderer have substantial native/platform code beyond the mostly safe application layer. Cargo advisories do not replace permission, sandbox, GPU, memory-pressure, and malformed-file tests. Imports have no app-specific size policy, and capture/display/export duplicate large image buffers; measure 4K/5K/8K behavior before setting supported limits.

## Native acceptance tests still required

These are release gates, not claims that every listed scenario currently fails.

| Area | Required evidence |
| --- | --- |
| Installation/lifecycle | Signed production-equivalent app/package on clean standard-user machines; first launch from store entry point; update; uninstall; preference preservation/migration; no reliance on developer tools or an installed C/C++ runtime that the package fails to supply. |
| Supported versions/architectures | Windows x64 and ARM64, Mac Apple Silicon and Intel if advertised; declared minimum OS and current OS. Check actual binary minimums and linked frameworks/DLLs. |
| Screen permission | Fresh grant, denial, revoked grant, later approval, restart/relaunch, and update preserving the intended identity. Visible recovery on every failure. |
| Monitors/capture | Primary/secondary, identical monitors, negative coordinates, unplug/replug, primary reassignment, Retina/1×, mixed scaling, portrait/flipped, SDR/HDR, cursor on/off, each supported Windows backend. Validate output dimensions and orientation. |
| Desktop integration | macOS Spaces/fullscreen/menu/Dock restoration; Start/Finder launch; app execution alias and documented hotkeys; overlapping launches; Windows Remote Desktop and unavailable capture/GPU cases. |
| Clipboard | Image pastes correctly after app exit into native image tools, office software, browsers, and messaging apps; temporarily locked clipboard; preserved transparency where supported; feedback when copying fails. |
| Files | Default/selected destination; sandbox authorization/bookmark persistence; redirected/OneDrive Pictures; Unicode user/path names; denied access; disk full; concurrent/repeated saves; malformed and very large PNGs. |
| Editing/export | Text-entry Save/Quit, IME/composition, fallback glyphs, transparent images, rotated annotations, undo/redo, crop edges, errors before selection, export during active gestures, high DPI/short screens, and output/preview agreement. |
| Accessibility | Keyboard-only tool/action navigation, focus visibility, VoiceOver/Narrator, named controls, large text/high contrast, discoverable quit/cancel, and accessible errors. egui/AccessKit presence alone does not prove the custom canvas usable. |
| Submission | Mac signature/entitlement/package validation; Windows package validation/WACK as applicable; install/update trials; limited Store/TestFlight testing; listing and review notes matching actual behavior. |

Store account readiness is outside the inspected repository: verify developer enrollment, reserved names/identities, agreements, support and privacy URLs, age/content ratings, screenshots, territories, applicable business disclosures, and signing/submission access. Existing README screenshots are useful draft material; validate platform appearance, required sizes, and metadata against the final app.

## Recommended remediation order

1. **Set the delivery baseline:** choose Windows MSIX versus installer, supported OS/architecture matrix, stable store identities, and version mapping. The recommended starting point is MSIX plus a separate sandboxed Mac App Store package.
2. **Prove native feasibility early:** build the signed sandboxed Mac app with authorized file access; fix the macOS API minimum and display metadata; choose/fix Windows capture handling. Run real capture/save/copy on both platforms before investing in listing polish.
3. **Fix confirmed user failures:** visible startup errors, collision-safe saves, toolbar height, alpha/font export, application shortcuts, and selection-independent error feedback.
4. **Complete distribution material:** icon assets, category/manifest, privacy/support access, store metadata, and packaged license notices.
5. **Make release promotion trustworthy:** bind every artifact to an immutable tested commit, gate on CI and native package validation, update/triage dependencies, and retain build provenance/symbols.
6. **Run the acceptance matrix and a limited store test release.** Public submission follows successful installation, permissions, capture/export, and update tests on the supported hardware/OS combinations.

Two policy distinctions prevent unnecessary work: Apple's current [required-reason API documentation](https://developer.apple.com/documentation/bundleresources/describing-use-of-required-reason-api) does not impose that blanket declaration rule on macOS, and the April 2026 Xcode 26 SDK deadline on [Upcoming Requirements](https://developer.apple.com/news/upcoming-requirements/) names other Apple platforms, not macOS. Assess any actual privacy-manifest/SDK obligations for the final dependencies and use supported submission tools; the present Mac CI runner label alone is not proof of a violation.
