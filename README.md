# DropAir

DropAir is currently a macOS cloud-build smoke test for a future Dropover-style
file shelf and transfer app.

The first validation target is deliberately small: GitHub Actions should build a
Tauri macOS `.app` bundle and upload it as `DropAir.app.zip` without requiring a
local Mac development environment.

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
