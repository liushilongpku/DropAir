# Runbook

## Local Validation

Run the frontend production build:

```sh
npm run build
```

Format Rust code with the cached toolchain used in this workspace:

```sh
RUSTUP_HOME=/tmp/dropair-rustup \
CARGO_HOME=/tmp/dropair-cargo \
/tmp/dropair-cargo/bin/cargo fmt --manifest-path src-tauri/Cargo.toml
```

Compile the Rust library and tests for macOS without linking a runnable app:

```sh
CC_x86_64_apple_darwin=/bin/true \
AR_x86_64_apple_darwin=/bin/true \
RUSTUP_HOME=/tmp/dropair-rustup \
CARGO_HOME=/tmp/dropair-cargo \
/tmp/dropair-cargo/bin/cargo check --tests --offline \
  --manifest-path src-tauri/Cargo.toml \
  --target x86_64-apple-darwin
```

Finish with:

```sh
git diff --check
git status --short --branch
```

If the cached Rust toolchain is absent, install or select an equivalent stable toolchain and macOS target. Do not interpret cross-target `cargo check` as AppKit runtime validation.

## Cloud Build

1. Commit the scoped changes and push `main`.
2. Confirm that `macOS Smoke Build` starts for the pushed commit.
3. Wait for the workflow to complete; inspect the failed job before making another change if it does not succeed.
4. Download the `DropAir.app.zip` artifact from the successful run.
5. Record the commit and Actions run URL in the handoff evidence when the build is a meaningful milestone.

For Windows, the `Windows Smoke Build` workflow runs on `windows-latest` and
uploads NSIS (`.exe`) and MSI (`.msi`) installers. Install the NSIS artifact for
the interactive regression pass; use the MSI artifact to verify enterprise-style
installation and uninstall behavior.

## macOS Installation

The artifact is ad-hoc signed and not notarized. If macOS reports that the application is damaged after download, run:

```sh
xattr -dr com.apple.quarantine /path/to/DropAir.app
```

Then right-click the application and choose Open.

## macOS Regression Checklist

1. Drag a file and shake outside DropAir; verify that Shelf appears near the pointer.
2. Drop files, folders, and selected text into the compact Shelf and main window.
3. Drag files and text back out without a crash.
4. Delete individual entries and clear all entries.
5. Add several entries, resize the Shelf, and verify scrolling.
6. Move/resize the Shelf, quit from the tray, restart, and verify entries and frame restoration.
7. Double-click a directory in both main and compact Shelf views; test Open and Show in Finder actions.
8. Switch Spaces and full-screen applications while Shelf is visible; verify it remains above the active application.
9. Toggle Shelf repeatedly with `Command+Shift+Space`.
10. Verify shake disable/enable, sensitivity levels, tray actions, login startup, main-window close/reopen, and single-instance activation.

## Windows Regression Checklist

1. Install the NSIS artifact and launch DropAir; verify the main window opens and the tray icon remains after closing it.
2. Drag files and folders into both the main window and Shelf; verify duplicate paths are ignored and text drops are retained.
3. Press `Ctrl+Shift+Space` from another application to show and hide Shelf.
4. Use the Shelf header to move the borderless Shelf and verify it stays above ordinary windows.
5. Open files and folders, and use the folder action to reveal them in Explorer.
6. Drag a Shelf file toward Explorer and verify the URI drop is accepted; record target applications that do not accept WebView URI payloads.
7. Delete individual items, clear all items, restart the app, and verify Shelf persistence.
8. Toggle launch at login, reopen the main window from the tray, and verify single-instance activation.
9. Install and uninstall the MSI artifact, then verify that the user data directory is not unexpectedly removed.
