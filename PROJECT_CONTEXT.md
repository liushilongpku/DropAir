# Project Context

Last verified: 2026-08-05 against commit `0f4778e` on `main`; Windows Smoke Build #6 succeeded.

## Purpose And Scope

DropAir is a Tauri 2 + React/TypeScript desktop utility that provides a Dropover-style temporary shelf on macOS and Windows. Its current usable scope is local file, directory, and selected-text collection. Device discovery and file transfer are planned but not implemented.

The development host is Linux/WSL and cannot execute or visually validate AppKit or Windows WebView behavior. macOS `.app` bundles and Windows installers are produced by GitHub Actions and manually tested on their target systems.

## Current Architecture

- `src/App.tsx`: both the main application UI and compact Shelf UI. The Shelf mode is selected with `?shelf=1`.
- `src/styles.css`: main window, settings, and compact Shelf styling.
- `src-tauri/src/lib.rs`: Tauri setup, application state, Shelf item persistence, commands, tray, autostart, single-instance handling, global shortcut, and lifecycle behavior.
- `src-tauri/src/shake_shelf.rs`: macOS global drag/shake detection, native `NSPanel`, cross-Space/full-screen behavior, native file drag-out, selected-text Drag Pasteboard capture, and Shelf frame persistence.
- `src-tauri/src/windows_shelf.rs`: Windows hidden Webview Shelf window, global shortcut visibility, tray visibility, and borderless window dragging.
- `src-tauri/src/settings.rs`: atomic JSON persistence for shake settings and Shelf frame.
- `.github/workflows/macos-smoke-build.yml`: macOS build, ad-hoc signing, ZIP packaging, and artifact upload.
- `.github/workflows/windows-smoke-build.yml`: Windows NSIS/MSI build and artifact upload.

## Stable Behavior

- A global macOS Core Graphics monitor detects horizontal shaking while the left button is held.
- The visible macOS Shelf is a native nonactivating `NSPanel`, not the hidden Tauri source window. It is movable, resizable, visible across Spaces/full-screen applications, and kept above ordinary windows.
- The visible Windows Shelf is a hidden-at-start borderless Tauri Webview window. It is movable through its header, kept above ordinary windows, omitted from the taskbar, and shown through the tray or global shortcut.
- Files and folders can be dropped into the Shelf. Existing files can be dragged out through AppKit's synchronous native drag API.
- Selected text is captured from the macOS Drag Pasteboard and can be dragged out as `text/plain`.
- Shelf entries are persisted to `shelf.json`; shake settings and Shelf frame are persisted to `settings.json` under Tauri's app config directory.
- Shelf entries support deletion and clearing. The main view can open a file/directory or reveal it in Finder/Explorer. Directories can also be double-clicked in the compact Shelf.
- The compact Shelf renders all entries and scrolls when its current size cannot show them all.
- `Command+Shift+Space` toggles the Shelf globally on macOS. Registration failure is logged and does not prevent application startup.
- Windows uses `Ctrl+Shift+Space` through the same `CommandOrControl+Shift+Space` registration. It has no shake monitor; the borderless Shelf is a separate always-on-top Webview window.
- Closing the main window hides it. The tray, Dock reopen event, and single-instance activation can restore it. Launch-at-login uses `--autostart` and starts silently.

## Native Constraints

- Operate on `SHELF_PANEL` for native Shelf behavior. Do not redirect native operations back to the hidden Tauri `shake-shelf` source window.
- Keep native file drag-out synchronous and initiated from the active mouse event. Previous asynchronous/spawn-blocking variants crashed or lost the drag session.
- Do not repeatedly reset `NSWindowCollectionBehavior`; configure it once when creating the panel. Repeated mutation previously caused crashes and full-screen regressions.
- Preserve the `NSPanel.contentView` path used by native file drag-out.
- File drags must be excluded before reading `NSPasteboardTypeString`, otherwise Finder drags can be misclassified as text.
- Frame persistence uses AppKit's bottom-left global coordinate system and debounced writes.

## Build And Distribution

- Local frontend validation: `npm run build`.
- Local Rust validation uses an installed macOS target and `cargo check --tests --target x86_64-apple-darwin`; this checks compilation but does not run AppKit behavior.
- Pushes to `main` start both `macOS Smoke Build` and `Windows Smoke Build`; the former uploads `DropAir.app.zip`, and the latter uploads NSIS and MSI installers.
- Bundles are ad-hoc signed, not notarized. Downloaded builds may require removing `com.apple.quarantine`, as documented in `README.md`.
- Windows installers are produced as NSIS and MSI artifacts by `.github/workflows/windows-smoke-build.yml`.
- Windows file drag-out uses WebView `text/uri-list`/`text/plain` payloads rather than the macOS native drag API; Explorer or other target applications may not accept them.
- `src-tauri/Cargo.lock` and generated Tauri files are intentionally not tracked in the current repository setup.

## Planned Direction

The next local-product work is richer item inspection, invalid-path handling, and undo/confirmation UX. The larger roadmap is authenticated device discovery and encrypted transfer over LAN, ZeroTier virtual LAN, and later WAN paths, followed by Windows/Linux clients.
