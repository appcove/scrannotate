# scrannotate

> [!WARNING]
> ## ⚠️ AI-generated code
> This project contains **AI-generated code**. It was an effort to solve a
> specific problem quickly, and **does not represent what we consider to be
> quality software code**. Read, review, and use accordingly.

**Screen. Annotate. Done.**

scrannotate is a fast, keyboard-friendly screenshot annotation tool for
Wayland. It grabs one screen per shot as **raw frames over PipeWire** — no
image encoding, no disk round-trip, so the editor is up in a blink. Select,
annotate, and ship — copy to the clipboard or save a PNG — without ever
leaving that one surface. No editor window, no dialogs, no save prompts.

![Annotating a capture: region selection, arrow, box, highlight, blur, marker, and text](docs/screenshot-annotate.png)

## Highlights

- **Screens are numbered slots.** `scrannotate` captures screen 1,
  `--screen 2` captures screen 2, and so on — the first use of a number
  shows the desktop's monitor chooser once and remembers your pick (a
  persisted portal grant per slot), so every run after that is instant and
  silent. Bind one hotkey per screen. `--pick-screen` re-binds a slot.
- **One surface, no modes to escape.** The capture fills the screen with
  full-screen crosshairs; drag out the region (its edges extend as guide
  lines while you drag) and the toolbar snaps in beside it. The region stays
  adjustable — with its own handles — right up until you copy. The status
  line at the toolbar's foot always explains what the current state means
  and what a click will do.
- **Every annotation stays live.** Nothing is baked in until export. With
  the Select tool, click any placed item to move it, resize it with handles,
  rotate it with the knob, restyle it, or delete it. Shift+drag a rubber
  band (or Ctrl+click) to select several and move, restyle, or delete them
  together. Undo/redo throughout — including region changes.
- **Ten tools**: Select, Pen, Line, Arrow, Box, Ellipse, Highlight, Blur
  (pixelate), Text, and auto-numbered Markers — drag a marker and an arrow
  grows out of it (one object; each end drags independently).
- **Inline text.** Text is typed directly on the image in its final font and
  color — no popup editor — and never soft-wraps; lines break only where you
  press Shift+Enter. Double-click any text, with any tool, to edit it again.
- **Settings that know their target.** The toolbar's settings section is
  labeled "For Current Object" or "For New Objects" and shows the target's
  actual color/width/size. A large color picker keeps your recently used
  colors one click away.
- **Pixel-identical export.** The PNG/clipboard renderer (tiny-skia) shares
  its geometry with the on-screen renderer, so what you see is exactly what
  you ship — including rotated shapes and rotated text.
- **Respectful of your flow.** Copy (`Enter`/`Ctrl+C`) puts the region on
  the clipboard and closes; Save (`Ctrl+S`) writes a PNG and closes; `Esc`
  steps back and double-`Esc` discards — never a confirmation dialog.
  Preferences (recent colors, deliberately-set sizes) persist between runs.

## Install

Linux with Rust 1.88+. Build dependencies:

```
sudo apt install libpipewire-0.3-dev clang pkg-config
```

(`cargo build --no-default-features` skips the PipeWire capture backend —
only `--from-file` works then; useful for developing the UI on a machine
without PipeWire headers.)

Runtime: PipeWire and xdg-desktop-portal (present on any GNOME, KDE, or
wlroots desktop), plus `wl-clipboard` — copying spawns `wl-copy`, whose
forked child keeps serving the clipboard after scrannotate quits (a Wayland
clipboard normally dies with its owner). Without it, copy falls back to an
in-process clipboard that only survives if a clipboard manager grabs it.

```
cargo build --release
install -Dm755 target/release/scrannotate ~/.local/bin/scrannotate
```

Bind it to your screenshot key (e.g. in GNOME: Settings → Keyboard →
Custom Shortcuts → `scrannotate` on `Print`).

## Usage

```
scrannotate                      # capture screen 1, select a region, annotate in place
scrannotate --screen 2           # capture screen 2 (first use: pick which monitor "2" means)
scrannotate --pick-screen        # re-open the monitor chooser to re-bind the slot
scrannotate --cursor             # include the mouse cursor in the capture
scrannotate --delay 3            # wait 3s before capturing (open that menu first)
scrannotate --save-path DIR      # where Ctrl+S saves (default ~/Pictures/Screenshots)
scrannotate --from-file img.png  # annotate an existing image (no capture)
```

The first use of each screen slot shows the desktop's screen-share dialog —
that's where you decide which monitor the number means; the granted portal
token is saved per slot, so subsequent captures skip it. (Wayland doesn't
let apps pick a monitor programmatically or ask where the cursor is, so the
one-time chooser is the deterministic way to bind numbers to screens.) Bind
hotkeys to taste: `Print` → `scrannotate`, `Shift+Print` →
`scrannotate --screen 2`.

### The flow

1. **Select** — the frozen frame appears with a crosshair under the cursor.
   Drag out the region; while dragging, the box edges extend across the
   whole screen so you can align both corners precisely. The region can be
   redrawn at any time (right-drag, with any tool) and moved or resized by
   its handles with the Select tool.
2. **Annotate** — the toolbar (draggable by its `• • •` grip) has the tools
   two per row, Arrow/Text/Marker/Line first. Pick one and draw. Markers
   drop with a click, or drag one to pull an arrow out of it. Text is typed
   inline; Shift+Enter for new lines.
3. **Refine** — tap `Space` (or `S`) for the Select tool: hover highlights
   what's clickable; click to select, drag to move, handles resize, the
   curved-arrow knob rotates, the four-arrow knob moves text, `Del`
   deletes. `Shift+drag` rubber-bands a multi-selection (`Ctrl+click` adds
   or removes one item, `Ctrl+A` selects everything); moving, restyling,
   and deleting apply to the whole selection. The settings panel edits
   whatever is selected — or the defaults when nothing is.
4. **Ship** — `Enter`/`Ctrl+C` copies the region and closes; `Ctrl+S` saves
   a PNG and closes; the toolbar also has plain Copy/Save buttons that keep
   the editor open. `Esc Esc` discards everything, no questions asked.

### Keys

| Tool | Key | | Action | Key |
|------|-----|-|--------|-----|
| Select | `S` / tap `Space` | | Copy & close | `Enter` / `Ctrl+C` |
| Pen | `P` | | Save & close | `Ctrl+S` |
| Line | `L` | | Undo / Redo | `Ctrl+Z` / `Ctrl+Shift+Z` |
| Arrow | `A` | | Delete selection | `Del` / `Backspace` |
| Box | `R` | | Multi-select | `Shift`+drag / `Ctrl`+click |
| Ellipse | `E` | | Select all | `Ctrl+A` |
| Highlight | `H` | | Reset view | `F` |
| Blur | `B` | | Cancel op → clear selection → Select tool → `Esc` `Esc` discards | `Esc` |
| Text | `T` | | Quit | `Ctrl+Q` |
| Marker | `M` | | Zoom / Pan | scroll / middle drag, `Space`+drag |
| | | | New region | right drag |
| | | | Reset all (back to region select) | `Shift+Esc` |

### The color picker

Click the color swatch in the toolbar's settings section:

![The color picker: saturation square, hue strip, labeled selected color, recent colors, OK/Cancel](docs/screenshot-picker.png)

Recently used colors form a most-recently-used stack (clicking one loads it
into the picker), and colors you actually draw with bubble to its head. The
recents — plus stroke width and text size once you've deliberately adjusted
them — persist in `$XDG_STATE_HOME/scrannotate/prefs`; untouched sizes stay
resolution-scaled defaults.

## How it works

Wayland doesn't let applications read the screen directly, so scrannotate
asks the **XDG Desktop Portal** for a `ScreenCast` stream
(`src/capture/screencast.rs`, via [pinray]/PipeWire) and takes a single
frame — raw RGBA over shared memory, no image encoding or disk round-trip,
which is why capture is fast. Each screen slot's grant persists via a
portal restore token, so only a slot's first use shows the chooser. The
frame is shown frozen in a fullscreen window; everything you do happens on
that frozen frame.

Inside the app, the frozen frame lives in a `Document` (annotations with
stable ids, the region, and transactional undo — `src/document.rs`), and
all pointer interaction runs through a single state machine
(`src/editor/state.rs`): exactly one interaction — drawing, region drag,
item drag, rubber-band, text edit, pan — can be active, so conflicting
drags are unrepresentable and every state defines its own Esc/cancel.
Annotations stay vector objects until export, when a **tiny-skia**
rasterizer that shares geometry with the on-screen egui renderer draws them
into the final PNG.

One workaround worth knowing about: pinray 0.2.4 only *logs* the portal
restore token instead of returning it, so `src/capture/screencast.rs`
catches it with a tracing layer. Drop that when pinray exposes the token
properly.

[pinray]: https://crates.io/crates/pinray

## Running inside a container (development)

Capture needs the host's **session D-Bus** socket (PipeWire itself arrives
as a file descriptor over the portal's `OpenPipeWireRemote`):

```
docker run ... \
  -v $XDG_RUNTIME_DIR/wayland-0:/run/user/1000/wayland-0 \
  -v $XDG_RUNTIME_DIR/bus:/run/user/1000/bus \
  -e WAYLAND_DISPLAY=wayland-0 \
  -e DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
```

Without the bus mount, `--from-file` still works (annotation only), and the
UI can be exercised headlessly under Xvfb — see
[CONTRIBUTING.md](CONTRIBUTING.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) — note that submissions transfer
their IP to the project (contributions are copyright-assigned, and the
project is distributed under Apache 2.0).

## License

[Apache License 2.0](LICENSE) — Copyright 2026 Jason Garber.
