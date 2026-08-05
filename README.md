# DropAir

DropAir is a macOS and Windows desktop file shelf for collecting files, folders,
and selected text before a later transfer step.

The first validation target is deliberately small: GitHub Actions should build
the Tauri macOS `.app` bundle and Windows installers without requiring a local
Mac or Windows development environment.

## Validate macOS Cloud Build

1. Push this repository to GitHub.
2. Open the **Actions** tab.
3. Run **macOS Smoke Build** manually, or push to `main`.
4. Download the `DropAir.app.zip` artifact.
5. Unzip it on macOS and open `DropAir.app`.

Because this smoke build is ad-hoc signed but not notarized, macOS may still
quarantine it after download. If macOS says the app is damaged, remove the
download quarantine attribute:

```sh
xattr -dr com.apple.quarantine ~/Downloads/DropAir.app
```

Adjust the path if you unzipped the app somewhere else, then right-click the app
and choose **Open**.

## Local Development

This workspace is WSL/Linux, so it cannot validate macOS AppKit behavior or
produce a macOS `.app` directly.

```sh
npm install
npm run build
```

## Windows Build

The Windows build provides a persistent Shelf window, tray controls, launch at
login, file/folder/text drop-in, and the global `Ctrl+Shift+Space` Shelf
shortcut. Windows does not use the macOS shake monitor; use the shortcut or the
tray menu to show Shelf.

GitHub Actions builds both NSIS and MSI installers with the **Windows Smoke
Build** workflow. The Windows Shelf uses the WebView drag payload for file
drag-out, so Explorer support depends on the target application's URI drop
handling.
