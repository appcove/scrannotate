# Capture backend patch

`pinray-platform-macos` is copied from the published 0.2.4 crate, upstream
commit `86097238059f38dd9f8d818bd6a513e7300aa9a2` of
https://github.com/Itz-Agasta/pinray. Its MIT license is retained in the crate.
Cargo applies this local crate through `[patch.crates-io]` for source/CI
builds of this repository.

The local changes are deliberately limited to:

- Deriving capture size from the selected display's actual backing scale.
- Reporting the primary display using `CGMainDisplayID`.
- Avoiding the macOS 13 audio selector for screenshot-only sessions on
  macOS 12.3, and returning an error for audio on unsupported systems.

Native testing must still verify 1×, Retina, scaled and rotated displays,
primary-display changes, and the declared minimum OS. Remove the patch after
an upstream release includes and verifies these behaviors. Cargo registry
publication does not preserve a root patch override: do not publish a new
crates.io application release until the upstream fix or equivalent dependency
arrangement is available.
