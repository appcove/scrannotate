# Contributing to scrannotate

Thanks for your interest in improving scrannotate! Bug reports, feature
ideas, and pull requests are all welcome.

## Contribution terms — please read first

By submitting a contribution to this project (a pull request, patch, code
snippet in an issue, or any other material intended for inclusion), **you
irrevocably assign all right, title, and interest in and to that
contribution — including its copyright — to Jason Garber**, and you:

1. represent that the contribution is your own original work (or that you
   otherwise have the right to assign it), and that it is not subject to any
   third-party license or obligation that would conflict with this
   assignment;
2. agree that the contribution may be relicensed, redistributed, and used
   without restriction at the copyright holder's discretion;
3. understand that the project is distributed to the public under the
   [Apache License 2.0](LICENSE), and your contribution will be made
   available under it.

If you cannot or do not wish to accept these terms, please do not submit
contributions; opening issues to report bugs or discuss ideas is still very
welcome.

Submitting a pull request constitutes acceptance of these terms.

## Development setup

Linux with Rust 1.88+ and:

```
sudo apt install libpipewire-0.3-dev clang pkg-config
```

Build and test:

```
cargo build
cargo test
cargo clippy --all-targets
```

PipeWire is only required by the `screencast` cargo feature (on by
default — it is the capture backend). On a machine without
`libpipewire-0.3-dev`, develop against the capture-less build (only
`--from-file` and the demo mode work):

```
cargo build --no-default-features
cargo test --no-default-features
cargo clippy --all-targets --no-default-features
```

Please keep `cargo clippy --all-targets` warning-free and `cargo test`
green; match the style of the surrounding code.

## Testing UI changes

The annotator can be exercised without a live capture:

```
cargo run -- --from-file some.png            # whole image preselected
```

## Regenerating the README screenshots

Screenshots are produced from a deterministic built-in demo scene, rendered
headlessly under Xvfb:

```
sudo apt install xvfb mesa-vulkan-drivers
Xvfb :99 -screen 0 1920x1200x24 -ac -nolisten tcp &
DISPLAY=:99 SCRANNOTATE_DEMO=annotate SCRANNOTATE_SHOT=docs/screenshot-annotate.png \
    cargo run   # unset WAYLAND_DISPLAY so winit picks X11
DISPLAY=:99 SCRANNOTATE_DEMO=picker SCRANNOTATE_SHOT=docs/screenshot-picker.png \
    cargo run
```

`SCRANNOTATE_DEMO=annotate|picker` seeds the canned scene;
`SCRANNOTATE_SHOT=<path>` saves a window screenshot once the UI settles and
exits. Demo mode never reads or writes the user's preferences.
