# Store-readiness implementation progress

This records implementation after the [original audit](store-readiness-audit.md).
The audit remains a historical snapshot. These changes prepare the code and
packaging process; they do not constitute a successful store submission.

## Finding disposition

| Finding | Implemented | Remaining acceptance or external work |
| --- | --- | --- |
| S1: Mac submission package | Parameterized signed `.app`/`.pkg` builder, sandbox entitlements, profile checks, icon/category/version metadata | Developer account, distribution profile/certificates, production icon, native validation and App Store Connect submission |
| S2: Windows Store route | MSIX builder with explicit Partner Center identity, x64/ARM64 checks, assets, full-trust capability and execution alias | Reserved identity/artwork, Windows SDK packaging, clean-machine runtime validation, WACK and Partner Center submission |
| S3: Sandbox file access | Native image/folder panels; retained security-scoped URLs; persistent output-folder bookmarks; cancellation and reauthorization | Test the installed signed sandbox on both Mac architectures, including moved/unavailable folders and repeated saves |
| S4: Privacy presentation | Offline policy and About view, support and optional compiled-in online policy link; policy included in packages | Review/publish the policy at a stable HTTPS URL, build with that URL, complete store privacy declarations |
| F1: Invisible startup failure | Native graphical-launch errors; capture retry, PNG fallback, permission guidance | Finder/Start validation, including renderer failure and permission denial/revocation |
| F2: Overwritten saves | Exclusive file creation with collision suffixes and cleanup on ordinary write failure | Disk-full and external-volume testing; a power failure can still leave an incomplete newly created PNG |
| F3: macOS 12.3 API mismatch | Avoid the macOS 13 audio selector in screenshot sessions; availability check for requested audio | Runtime test on minimum supported macOS; track upstream replacement of local patch |
| F4: macOS display metadata | Real primary display and display-mode backing scale | Retina, 1×, scaled/rotated, mixed-DPI and primary-display changes on native hardware |
| F5: Windows backend mismatch | Explicit WGC; documentation and package floor aligned to Windows 10 2004; no incorrect automatic DXGI fallback | Native rotation/cursor/permission coverage; HDR color fidelity remains limited |
| F6: Oversized toolbar | Bounded scrolling toolbar/picker with persistent status and confirmation controls | Native display scaling checks; egui layout regressions cover 960×540 logical size |
| F7: Transparent PNG colors | Straight RGBA base preserved, premultiplied annotation overlay composited correctly, alpha-aware pixelation | Real clipboard receivers on Windows/macOS |
| F8: Font fallback mismatch | Shared egui font layout/fallback for export, including rotated text; old pixel-identical claim removed | GPU/CPU antialiasing can differ; bundled fonts still limit glyph coverage |
| F9: Text-mode lifecycle shortcuts | Save commits text; Quit works while typing; failures keep editor open | Native Command/Ctrl integration; tests cover application state transitions |
| F10: Store launch controls | Visible feedback before selection, native Open PNG / Save folder, Windows Known Folder default, About/help | Screen choice, capture delay and cursor inclusion remain command-line options; richer capture-launch settings are still deferred |
| R1: Ungated publication | Release calls six-target CI for the selected SHA before upload/publication | Run changed workflows on GitHub; local checks cannot validate hosted runners |
| R2: Mismatched draft source | Full SHA required; draft and peeled tag must match before resume/publish | Follow documented recovery process; do not move release tags |
| R3: Missing notices | Target/feature-aware dependency and font texts included in archives and app bundles, missing texts fail packaging | Review distribution obligations and documented canonical-license exceptions |
| R4: Build prerequisites | Rust 1.92 minimum, pinned 1.96.1 toolchain/actions, locked builds, source/compiler metadata and corrected contributor docs | SDK/image pinning, symbol retention and byte-for-byte reproducibility remain future work |

An additional export guard rejects saving/copying during an unfinished pointer
operation, so an in-progress blur cannot be silently omitted. Open cancellation
and decode failures preserve the current document; successful replacement
requires confirmation when edits would be discarded.

## Dependency review

The lockfile updates `event-listener`, `webbrowser`, and `wayland-scanner`
(and its `quick-xml` dependency). Direct dependencies on the old export font
rasterizer were removed. A fresh `cargo audit` reports zero known
vulnerabilities and one maintenance warning: `ttf-parser 0.25.1`
(`RUSTSEC-2026-0192`). Its remaining path is Linux window decoration through
`sctk-adwaita`/`winit`; it is absent from the Windows and macOS dependency trees.
No advisory is suppressed. Keep checking the advisory database when releasing.

The macOS backend is a licensed, documented local source patch. Do not publish
the application through Cargo's registry until that patch is upstreamed or an
equivalent dependency arrangement is in place; registry publication does not
preserve root patch overrides. See [patch provenance](../vendor/README.md).

## Validation and next steps

Completed on the Linux development host:

| Check | Result |
| --- | --- |
| `cargo test --locked` | 63 tests passed |
| `cargo test --locked --no-default-features` | 63 tests passed |
| `cargo clippy --all-targets --locked -- -D warnings`, also without default features | Passed |
| Cross-target Clippy, including test compilation, for Windows x64/ARM64 and macOS Intel/Apple silicon | Passed; both Mac targets also checked with `mac-app-store` |
| Python release/store packaging suite | 12 tests passed, including PowerShell execution with mocked MakeAppx |
| `actionlint`, Bash syntax, ShellCheck and Python compilation | Passed |
| Dependency notices for all six release targets and both Mac Store targets | Generated successfully |
| `cargo audit` | Zero known vulnerabilities; one Linux-only maintenance warning described above |
| Linux debug build, `--help`, `--build-info`, redirected invalid-argument and missing-file exits | Passed |
| Xvfb demo render | Screenshot generated and visually inspected |

These checks exercise local application logic and packaging boundaries. They
do not execute Windows or macOS application binaries, signing tools, MakeAppx,
or store certification. Formatting is not a CI gate while the pre-existing
repository formatting baseline remains unnormalized.

Before a store submission:

1. Supply the account identities, production icons and published privacy URL
   described in [store packaging](store-packaging.md).
2. Build/sign on native systems using the pinned source and feature settings.
3. Complete [native file-access checks](native-file-access.md) and
   [capture/platform checks](capture-platforms.md), then validate installed
   packages, updates and uninstall through the appropriate store test channels.
4. Complete store metadata, privacy declarations, screenshots, age rating and
   capability explanations; submit only after those acceptance checks pass.

No live release was published, developer identity created, signing credential
changed, or store package submitted during this implementation.
