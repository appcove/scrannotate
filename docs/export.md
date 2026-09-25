# Image export

Save and Copy share `export::render_to_image`, so both receive the same cropped,
straight-alpha RGBA pixels. Saving encodes those pixels as a PNG; Copy passes
them to the platform clipboard implementation.

## Saving without overwriting

The first save uses `scrannotate-YYYY-MM-DD_HHMMSS.png` in the chosen output
directory. If that name already exists, subsequent saves use `-1.png`,
`-2.png`, and so on. The directory is created if needed.

Each filename is reserved with exclusive file creation. There is no separate
existence check, so concurrent application instances cannot reserve the same
name or overwrite an existing file. Encoding, buffered flush, and file sync
must succeed before Save reports success. An ordinary write/encoding failure
closes and removes its incomplete file. A cleanup failure includes the file's
path in the error so the user can find it.

An abrupt process termination or power failure can still leave an incomplete
file; these saves are not a filesystem transaction. A later save skips that
existing name instead of overwriting it.

## Transparency

PNG imports and clipboard output use straight RGBA. The vector rasterizer uses
premultiplied RGBA. Annotations are therefore drawn into a transparent overlay
and composited over the original image with source-over blending. Pixels that
the overlay does not touch retain their original bytes, including low-alpha
colors and RGB values hidden by zero alpha. This avoids quantization damage
from converting the entire imported image to 8-bit premultiplied RGBA and back.

Pixelation is applied to the base image before vector annotations, as in the
preview. Its block averages weight color by alpha and average alpha separately.
Transparent pixels no longer become opaque or contribute hidden colors to the
visible mosaic. Fully transparent mosaic blocks have zero RGBA. Opaque image
blocks retain the previous integer-average behavior.

The renderer uses 8-bit channels; pixels changed by annotations can still incur
normal rounding error. Image dimensions also affect working memory: export
currently retains the base copy and a full-sized annotation overlay.

## Text and preview agreement

Text export uses egui/epaint's font definitions, proportional fallback chain,
shaping, line layout, glyph atlas, and pixel-snapped glyph origins. This includes
the embedded emoji fallback used by the canvas. Marker numbers use the same
layout and centering. Rotated text rotates the laid-out block about the same
galley center used by the preview.

Export lays out at one image pixel per point rather than using the current
display scale or canvas zoom. GPU sampling, preview zoom, and theme-dependent
font antialiasing can affect edge pixels; export is not guaranteed to be
pixel-identical to a screenshot of the UI. Characters unavailable in egui's
embedded fonts still use its replacement glyph. This change does not add
system fonts, color emoji, or universal script coverage.

If the application starts installing custom fonts or changes text options,
update the export font initialization together with the UI configuration.

## Regression checks

Run the focused checks with:

```sh
cargo test --locked --no-default-features export::
cargo test --locked --no-default-features annotate::tests
```

They cover repeated and concurrent saves with a fixed timestamp, preservation
of existing files, partial-write cleanup, unchanged pixels at all 256 alpha
values, transparent and semitransparent highlights, PNG roundtrips, text and
rotated-text alpha, distinct fallback emoji, all annotation kinds, and
transparent/opaque pixelation. Native clipboard consumers and GPU preview
agreement still need testing on Windows and macOS.
