# Native capture corrections

Windows uses `BackendPreference::WindowsWgc` explicitly. Pinray 0.2.4's
automatic selection can prefer DXGI, whose current implementation lacks
display-rotation and separate hardware-cursor composition. A WGC failure
now reaches the application's recovery dialog instead of silently selecting
that backend. Windows 10 2004 is the supported floor for cursor exclusion.
Capture borders remain subject to OS permissions; HDR-to-SDR color fidelity
is still a limitation.

The macOS capture crate is patched locally; [vendor/README.md](../vendor/README.md)
records its upstream revision and license. Capture dimensions derive from
the selected display's backing scale rather than a fixed 2× multiplier.
Primary-display metadata comes from `CGMainDisplayID`. Screenshot sessions
avoid the audio setter introduced after macOS 12.3, and audio requests check
selector availability before using it. The deployment target remains 12.3.

The main executable reports startup errors in a native dialog for graphical
launches, including UI initialization failures. Capture failures offer retry,
native PNG selection, or quit, with platform permission guidance. Terminal
launches preserve stderr diagnostics and a failure exit status. Any standard
stream attached to a terminal suppresses native error/recovery dialogs;
fully redirected scripts can use `--no-dialogs`. Default
Windows output uses the system Pictures Known Folder.

## Validation still required on installed builds

- Windows x64 and ARM64: portrait/landscape, multiple displays, mixed DPI,
  cursor included/excluded, denied capture, disconnected display, and HDR.
- macOS Intel and Apple silicon: 1×/Retina/scaled and rotated displays,
  changing primary display, capture-permission denial/revocation, and the
  oldest supported OS on compatible hardware.
- Both: Finder/Start error dialogs without a terminal, retry after granting
  access, PNG fallback, window placement, clipboard lifetime after closing,
  and installation/uninstallation through the intended store package.

Cross-target `cargo check` validates conditional Rust code and native bindings,
not capture pixels, GPU behavior, permissions, signing, or minimum-OS runtime
compatibility. Do not represent those acceptance checks as completed on Linux.
