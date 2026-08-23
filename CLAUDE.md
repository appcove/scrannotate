# CLAUDE.md

Guidance for Claude Code (and humans) working in this repository.

## What this is

`scrannotate` is a fast, keyboard-friendly screenshot annotation tool for
**Linux (Wayland), macOS, and Windows**, written in Rust with an egui/eframe
UI. It captures one screen per shot as a raw frame (no encoding, no disk
round-trip), then lets you select a region and annotate it in place —
copy to the clipboard or save a PNG, no editor window or dialogs.

`README.md` is the user-facing manual and is unusually complete — read its
**How it works** and **Platform notes** sections before touching capture,
export, or platform code. This file is the contributor/agent orientation.

## Build, test, lint

```bash
cargo build                       # debug build (dev profile is opt-level 1;
                                  # image/pixelate paths are unusably slow at 0)
cargo test --locked               # unit tests (fast, no display needed)
cargo clippy --all-targets --locked -- -D warnings   # must stay warning-free
cargo build --release --locked    # release build
```

CI (`.github/workflows/ci.yml`) runs exactly `clippy --all-targets`,
`test`, and a release build on six targets: Linux x86_64+arm64 (ubuntu-24.04),
macOS arm64+Intel, Windows x86_64+arm64. **Keep clippy clean and tests green
on every platform** — that is the contribution bar (see `CONTRIBUTING.md`).

### The `capture` feature

`capture` is on by default and carries the `pinray` capture backend on every
platform. On **Linux** it needs system packages at build time:

```bash
sudo apt install libpipewire-0.3-dev clang pkg-config   # PipeWire 1.x (Ubuntu 24.04+)
```

macOS and Windows need no system packages. To develop the UI without the
backend (only `--from-file` and demo mode work):

```bash
cargo build --no-default-features
cargo test  --no-default-features
cargo clippy --all-targets --no-default-features -- -D warnings
```

### Running / exercising the UI

```bash
cargo run -- --from-file some.png    # annotate an existing image, no capture
cargo run -- --pick-screen           # macOS/Windows: list screens; Linux: re-bind a slot
```

Demo mode renders deterministic scenes for the README screenshots without
touching real screens or user prefs — see `CONTRIBUTING.md` §"Regenerating
the README screenshots" (`SCRANNOTATE_DEMO` / `SCRANNOTATE_SHOT`).

## Architecture

One fullscreen frozen-frame surface; everything happens on that one frame.
Data flows capture → `Document` → `Editor` state machine → `ui` painters →
`export` rasterizer.

| Module | Responsibility |
|--------|----------------|
| `main.rs` | CLI (clap), capture-vs-file dispatch, eframe/viewport setup, platform window quirks (macOS simple-fullscreen, Windows console reattach). |
| `capture/` | One monitor per shot via `pinray`. `screencast.rs` = Linux portal/PipeWire; `monitor.rs` = macOS ScreenCaptureKit / Windows Graphics Capture. `session.rs` = the shared one-shot session. Gated by the `capture` feature. |
| `app.rs` | The eframe shell: owns the `Editor`, display texture, global shortcuts, save/copy, toasts, double-Esc discard, prefs persistence. |
| `editor/` | egui-free interaction controller. `mod.rs` turns pointer/key input into transitions; `state.rs` is the **single interaction state machine** (exactly one of draw/region-drag/item-drag/rubber-band/text-edit is ever active — conflicting drags are unrepresentable); `geometry.rs`, `hit.rs` are math/hit-testing. Unit-testable because text extent enters through a `Measure` closure. |
| `document.rs` | Base image + annotations (addressed by stable `AnnotationId`, never index) + region + **transactional** snapshot undo/redo (`begin`/`commit`/`rollback`). |
| `annotate.rs` | Annotation model and the geometry **shared** by the on-screen (egui) and export (tiny-skia) renderers. All coords are image-pixel space. |
| `ui/` | All painting: `canvas.rs`, `toolbar.rs`, `paint.rs`, `color_picker.rs`, `text_overlay.rs`. No interaction logic. |
| `export.rs` | tiny-skia rasterizer for save/copy; shares geometry with `annotate` so PNG output is pixel-identical to the screen. |
| `prefs.rs` | Tiny `key=value` file (recent colors; stroke/font size only once deliberately changed) in the platform state dir. |
| `clipboard.rs`, `view.rs` | Clipboard put (Linux forks `wl-copy`); pan/zoom transform. |

### Invariants worth preserving

- **Geometry lives once, in `annotate.rs`.** The egui renderer (`ui/`) and
  the tiny-skia renderer (`export.rs`) both read it, which is why exports are
  pixel-identical. Don't fork geometry into either renderer.
- **Undoable mutations run inside a transaction.** `doc.begin()` before the
  first change, `doc.commit()` on pointer-up, `doc.rollback()` on Esc.
- **Exactly one interaction is active** (`EditorState`). New interactions are
  new enum variants, not new parallel `Option` fields.
- **Annotations are addressed by `AnnotationId`, never by index** — a stale id
  resolves to nothing, not to the wrong object.
- **A region must exist before annotations can be placed.** Drawing tools'
  first drag draws the region; text/marker clicks are ignored until one
  exists (the toolbar that would show them doesn't appear yet either).

## Releases, signing, publishing

Version in `Cargo.toml` is the source of truth. Pushing a `Cargo.toml`
version bump to `main` triggers `.github/workflows/release.yml`, which
tags, drafts a GitHub Release, fills in binaries for six targets, and
publishes.

- **macOS bundling**: `packaging/macos/bundle.sh` assembles, signs, and zips
  `scrannotate.app`. `./build.sh` runs it locally (`--universal` for a fat
  binary). Direct-download builds are currently **ad-hoc signed** only.
- **Store distribution** (Mac App Store, Microsoft Store MSIX):
  [`docs/PUBLISHING.md`](docs/PUBLISHING.md) is the ordered click-by-click
  runbook; [`docs/SIGNING.md`](docs/SIGNING.md) is the account/cert/secret
  reference. Packaging scripts live under `packaging/`.

## Conventions

- Match the surrounding style. This codebase favors **dense, explanatory
  comments** that state the *why* (platform quirks, invariants, workarounds),
  not the *what*. New non-obvious code should carry the same.
- Rust edition 2024, MSRV 1.88+.
- Platform-specific code is `#[cfg(...)]`-gated; keep the three platforms at
  parity and test all six CI targets in mind.
- No `unwrap()`/`expect()` on fallible I/O in real paths — use `anyhow` with
  `.context(...)`.
