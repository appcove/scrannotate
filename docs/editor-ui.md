# Editor controls and help

The toolbar appears after you select a capture region. Drag its dotted grip to move it beside your work. On smaller screens or at higher display scaling, scroll within the toolbar to reach all tools and the Copy, Save, and Close actions. The status message stays visible below the scrolling controls. The color picker also scrolls, with its OK and Cancel buttons kept visible.

Save and clipboard errors appear even before a region is selected. The first Escape at the end of the editor's cancellation steps shows a discard confirmation; press Escape again promptly to discard and close.

Toolbar shortcut labels follow the operating system: Command on macOS and Ctrl on Windows/Linux. Command/Ctrl+S saves and closes; Command/Ctrl+C copies and closes. While typing, normal text-copy behavior takes precedence when text is selected or the caret is not at the end. Command/Ctrl+Z undoes and Command/Ctrl+Y redoes.

Save commits active text, including when invoked while typing. Command/Ctrl+Q and Command/Ctrl+W also work during text editing. Saving or copying during an unfinished drag asks you to finish or cancel it first, so an incomplete blur cannot be omitted silently. Failed exports keep the editor open. Application export shortcuts are suspended while the privacy dialog is open.

**About & Privacy** is available in the upper-right corner, including before region selection. It opens the privacy policy, application license, third-party notices, and a support link. Release packages include dependency notices; development builds may not have them. Store builds also provide a link to the published privacy policy. Escape, the dialog's Close button, or a click on its backdrop dismisses the dialog.

## Regression checks

Run `cargo test --no-default-features ui::` to exercise the UI without capture dependencies. The tests run real egui layout and input frames on a 960×540 logical canvas, equivalent to a 1920×1080 display at 200% scaling. They check toolbar bounds, scrolling and clicking Save, the fixed status footer, color-picker confirmation buttons, feedback without a selected region, and the privacy dialog's bounds. Native Windows/macOS testing is still needed for display scaling and keyboard integration.
