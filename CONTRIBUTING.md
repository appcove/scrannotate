# Contributing to scrannotate

Thanks for your interest in improving scrannotate! Bug reports, feature
ideas, and pull requests are all welcome.

## Contribution terms — please read first

By submitting a contribution to this project (a pull request, patch, code
snippet in an issue, or any other material intended for inclusion), **you
irrevocably assign all right, title, and interest in and to that
contribution — including its copyright — to AppCove, Inc.**, and you:

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

Rust 1.92+ on Linux, macOS, or Windows. The repository pins Rust 1.96.1
for development and CI in `rust-toolchain.toml`. On Linux the capture backend
builds against PipeWire (1.x — e.g. Ubuntu 24.04+):

```
sudo apt install libpipewire-0.3-dev clang pkg-config
```

macOS and Windows need no system packages.

Build and test:

```
cargo build --locked
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

PipeWire is only required on Linux, and only by the `capture` cargo
feature (on by default — it carries the capture backend on every
platform). On a Linux machine without `libpipewire-0.3-dev`, develop
against the capture-less build (only `--from-file` and the demo mode
work):

```
cargo build --no-default-features
cargo test --no-default-features
cargo clippy --all-targets --no-default-features
```

Please keep `cargo clippy --all-targets` warning-free and `cargo test`
green on every platform; match the style of the surrounding code. CI
(`.github/workflows/ci.yml`) runs clippy, build, and tests on Linux
(x86_64 + arm64), macOS, and Windows (x86_64 + arm64) for every PR.
Releases are prepared by `.github/workflows/release.yml` on qualifying pushes
to `main` or manual dispatch. The package version selects the release;
all native validation jobs must pass for the same immutable source commit
before packaging and publication. See [release procedures](docs/releases.md).

Mac App Store file authorization is behind `mac-app-store`; on a Mac also run
`cargo test --locked --features mac-app-store` and
`cargo clippy --locked --all-targets --features mac-app-store -- -D warnings`.
Test installed sandboxed packages using the
[native acceptance checks](docs/native-file-access.md). Cross-compilation
does not replace these checks. Packaging entry points and required signing
inputs are documented in [store packaging](docs/store-packaging.md).

## Testing UI changes

The annotator can be exercised without a live capture:

```
cargo run -- --from-file some.png            # whole image preselected
```

## Regenerating the README screenshots

Screenshots come from deterministic built-in demo scenes.
`SCRANNOTATE_DEMO=<mode>` seeds a canned scene — `annotate`, `multiselect`,
`text`, or `picker`, one per README image — and `SCRANNOTATE_SHOT=<path>`
saves a window screenshot once the UI settles, then exits. Demo mode never
reads or writes the user's preferences. Regenerate all four with:

```
for mode in annotate multiselect text picker; do
    SCRANNOTATE_DEMO=$mode SCRANNOTATE_SHOT=docs/screenshot-$mode.png \
        cargo run
done
```

Any session that can open a window works (capture isn't involved, so
`--no-default-features` is fine too). On a headless machine, render under
Xvfb — unset `WAYLAND_DISPLAY` so winit picks X11:

```
sudo apt install xvfb mesa-vulkan-drivers
Xvfb :99 -screen 0 1920x1200x24 -ac -nolisten tcp &
DISPLAY=:99 SCRANNOTATE_DEMO=annotate ... cargo run
```
