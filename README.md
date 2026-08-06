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
login, file/folder/text drop-in, horizontal-shake detection, and the global
`Ctrl+Shift+Space` Shelf shortcut. Hold the left button and shake the mouse
horizontally to show Shelf, or use the shortcut or the tray menu.

GitHub Actions builds both NSIS and MSI installers with the **Windows Smoke
Build** workflow. The Windows Shelf uses the WebView drag payload for file
drag-out, so Explorer support depends on the target application's URI drop
handling.

## LAN Transfer (preview)

DropAir discovers other instances on the same local network and can send Shelf
files and text between devices:

- Discovery: every instance broadcasts its identity over UDP port `47653`.
- Transfer: files and text are streamed over TCP port `47654`.
- Received files and text are stored under DropAir's app data `received`
  directory and added to the local Shelf automatically.

Open **Devices** in the main window to see discovered devices and send the
current Shelf items to a selected device. The main toolbar **Send** button uses
the selected device (or the first device found).

Limitations of this preview:

- Transfers are unencrypted and unauthenticated; use it only on trusted LANs.
- Directory and "other" Shelf items are skipped; files and text are supported.
- Discovery uses subnet broadcast, so devices on different subnets or over WAN
  are not found yet. ZeroTier virtual LAN support is planned.
- On Windows, the first inbound transfer may trigger a firewall prompt; allow
  DropAir on private networks.

Pairing, encryption, and WAN transport are the next milestones.

### Troubleshooting macOS to Windows transfers

If sending from macOS fails with `Connection refused`, the Windows machine is
not accepting inbound TCP connections on port `47654`. Allow DropAir through
Windows Defender Firewall for private networks, or add an inbound rule from an
administrator PowerShell:

```powershell
netsh advfirewall firewall add rule name="DropAir" dir=in action=allow protocol=TCP localport=47654 profile=private
netsh advfirewall firewall add rule name="DropAir Discovery" dir=in action=allow protocol=UDP localport=47653 profile=private
```

The **Devices** page shows whether the transfer listener is running.
