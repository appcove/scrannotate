# Native image and output-folder access

Windows and macOS builds provide **Open PNG…** and **Save folder…** controls in the upper-left corner, including before a region is selected. These controls use native dialogs to open PNG images and choose a screenshot output folder. Cancelling a dialog leaves the current image and destination unchanged. The image is fully decoded while its file authorization is active. Ordinary desktop builds use the selected output directory for the current app session. Cancelling Save keeps the editor open, including when Save+Close was requested.

Opening another image asks for discard confirmation if there are annotations, region edits, or an active text/pointer interaction. The current document is replaced only after the chosen PNG has decoded successfully. The imported PNG starts fully selected, with a fresh view and undo history; color, stroke width, text size, and palette preferences carry over.

Build macOS store packages with the `mac-app-store` Cargo feature. In this build, the first Save opens an output-folder chooser. Scrannotate remembers the selected folder using a security-scoped bookmark at `prefs::state_dir()/output-folder.bookmark`. Later saves and later app launches resolve that bookmark and hold the authorization until writing finishes. Selecting another output folder replaces the remembered authorization. The suggested directory, including a command-line path, only sets the initial dialog location; it does not grant file access or override a remembered selection.

If the remembered permission cannot be restored, or the folder is unavailable, Scrannotate explicitly asks for a folder again. Cancelling keeps the previous bookmark and does not save anywhere else. A valid stale bookmark is refreshed while its folder is authorized. Bookmark replacement is atomic, and persistence failures are reported before a screenshot is written to a newly selected folder.

The signed Mac app requires these entitlements:

- `com.apple.security.app-sandbox`
- `com.apple.security.files.user-selected.read-write`
- `com.apple.security.files.bookmarks.app-scope`

The store-specific picker uses AppKit directly so it can retain the actual URL returned by the panel. The `rfd` path-only API used by ordinary builds does not retain that URL. The RAII guard releases a panel's implicit grant or a successfully started bookmark grant exactly once, including on image decoding, persistence, or write failures. This follows [Apple's sandbox file-access guidance](https://developer.apple.com/documentation/security/accessing-files-from-the-macos-app-sandbox).

## Native acceptance tests still required

Cross-target compilation validates the Rust bindings and feature configuration. It does not validate sandbox behavior, native panels, or signing. Run the following against the installed, signed sandboxed app on a Mac:

1. Save for the first time, choose an external folder, and verify the image appears there. Cancel the first chooser and verify the editor stays open without creating a file.
2. Relaunch and save again without another chooser. Repeat many times to check that authorization scopes are released without exhausting sandbox extensions.
3. Rename or move the selected folder, relaunch, and verify the resolved destination and refreshed bookmark. Disconnect a volume or remove folder access and verify explicit reauthorization with no save to a guessed fallback directory.
4. Change the output folder, cancel a later change, then relaunch and confirm the last successful selection remains in use.
5. Try an unwritable folder, a full volume, and an unwritable app settings directory. Verify clear errors and that editing remains available.
6. Open a PNG outside the app container, including Unicode filenames. Verify decoding succeeds before authorization is released. Cancel and choose an invalid or damaged PNG; verify the current document remains unchanged.
7. Validate the same dialogs from the fullscreen editor and during text editing. Confirm they appear above the editor, receive focus, and return focus afterwards.

On Windows and ordinary macOS builds, verify PNG filtering, cancelling, folder changes, Unicode paths, write failures, and saving to the selected session directory.

## Automated application regressions

The application tests cover Save cancellation without closing, failed Save after text commit, preservation of the current document and text when Open is cancelled or fails, the discard-confirmation predicate, and replacement resetting document/texture/selection while retaining style preferences. These tests inject operation results or exercise the file writer directly; they do not display native dialogs, including under `--all-features` on macOS. Desktop UI tests also verify Open and Save folder are reachable before selecting a region and remain separate from the toolbar.
