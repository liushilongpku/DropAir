# Operations Record

## Recent Operations

### 2026-08-05 | Windows Shelf implementation
- What changed: Added a Windows-specific Shelf Webview window, `Ctrl+Shift+Space` visibility toggle, tray integration, Windows path open/reveal commands, platform capability reporting, Windows URI drag payloads, and NSIS/MSI GitHub Actions packaging.
- Why: Extend the local file shelf from its macOS-first implementation to a usable Windows desktop version while preserving the macOS native shake and drag paths.
- Result and impact: The shared React UI now hides macOS-only shake/accessibility controls on Windows and keeps persistence, drop-in, item actions, launch-at-login, and single-instance behavior cross-platform.
- Evidence: `src-tauri/src/windows_shelf.rs`, `.github/workflows/windows-smoke-build.yml`, `npm run build` passed; [Windows Smoke Build #6, run 30997498602](https://github.com/liushilongpku/DropAir/actions/runs/30997498602) succeeded and uploaded `DropAir-windows-installers`.
- Next direction: Install the Windows artifact and complete the Windows regression checklist in `RUNBOOK.md`, then decide whether Explorer compatibility requires native COM `DoDragDrop` support.

### 2026-07-29 | Native selected-text support
- What changed: Added text Shelf items and replaced unreliable WebView-only text drops with native macOS Drag Pasteboard capture while preserving file URL handling.
- Why: Tauri's native file drop integration can prevent React from receiving ordinary `text/plain` drop events.
- Result and impact: The user confirmed text dragging works; cloud build #23 succeeded.
- Evidence: Commits `26edf6a`, `185ec32`; [Actions run 30437852090](https://github.com/liushilongpku/DropAir/actions/runs/30437852090).

### 2026-07-29 | Persistent settings and configurable shake detection
- What changed: Added shake enable/disable, five sensitivity levels, accessibility status/settings access, atomic settings persistence, and Shelf position/size restoration.
- Why: Make global shake behavior adjustable and preserve the native Shelf layout.
- Result and impact: GitHub macOS Smoke Build #21 completed successfully and the settings architecture remains the active implementation.
- Evidence: Commit `4c6e928`; [Actions run 30434731350](https://github.com/liushilongpku/DropAir/actions/runs/30434731350).

### 2026-07-29 | Menu bar and stable native drag-out
- What changed: Added tray and launch-at-login lifecycle support, then restored native file drag-out through a synchronous `NSPanel.contentView` operation.
- Why: Match normal macOS background-app behavior without reintroducing the drag-out crash.
- Result and impact: The user confirmed the file drag-out fix; cloud build #20 succeeded.
- Evidence: Commits `f166063`, `c7027e4`; [Actions run 30430723397](https://github.com/liushilongpku/DropAir/actions/runs/30430723397).

### 2026-07-29 | Compact Shelf directory opening
- What changed: Added directory double-click opening to the compact Shelf and stopped delete-button double-click propagation.
- Why: Directory opening previously worked only in the main window.
- Result and impact: Frontend build and GitHub macOS Smoke Build #25 succeeded; macOS interaction regression testing remains pending.
- Evidence: Commit `3bbc3fc`; [Actions run 30446415364](https://github.com/liushilongpku/DropAir/actions/runs/30446415364).

## Historical Summary

- 2026-07: Established the native Core Graphics shake monitor and real AppKit `NSPanel`, then stabilized all-Space/full-screen visibility, close/reopen behavior, file drop-in, and synchronous file drag-out through commits ending at `39bf9ca` and `c7027e4`.
