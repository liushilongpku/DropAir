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

## In-App Updates

DropAir uses the Tauri updater against GitHub Releases. Once a release with
updater artifacts exists, the app checks for a newer version on launch and can
install it from **Settings -> Check for updates** (or the "Install update"
button when one is available). The update is downloaded and applied in place, so
no manual download, re-signing, or replacement is needed for later versions.

The very first install still has to be done manually from a release asset and
may require removing the quarantine attribute as described above, because the
build is ad-hoc signed and not notarized.

### Publishing a new version

1. Bump `"version"` in `src-tauri/tauri.conf.json` (and keep `package.json` in
   sync), e.g. to `0.2.0`.
2. Commit and push to `main`.
3. Push a matching tag:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

The **Release** workflow builds macOS (Apple Silicon + Intel) and Windows
artifacts, signs the updater bundles with the `TAURI_SIGNING_PRIVATE_KEY`
repository secret, uploads them to a draft GitHub Release, and then publishes
it. The app's updater endpoint is
`https://github.com/liushilongpku/DropAir/releases/latest/download/latest.json`.

> Keep the update signing private key safe. It was generated at
> `~/.tauri/dropair.key` and stored as the `TAURI_SIGNING_PRIVATE_KEY` Actions
> secret. If it is lost, already-installed apps can no longer be updated.


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
- Received files are stored in the configurable **Download location** from
  Settings. The default is `Downloads/DropAir`; received text is restored as a
  text Shelf item.

Open **Devices** in the main window to see discovered devices and send the
current Shelf items to a selected device. The main toolbar **Send** button uses
the selected device (or the first device found).

Device discovery runs automatically in the background. **Scan now** sends an
immediate discovery announcement when a device list refresh is needed. If
broadcast discovery is blocked, use **Add a device manually** with the target
IP address or resolvable host name and TCP port. Manual entries remain available
until removed and are replaced by an automatic entry when the same endpoint is
later discovered.

Discovered devices can be linked explicitly with **Link**. The device list
merges multiple addresses announced by the same DropAir identity, so a machine
with both `192.*` and `10.*` interfaces stays as one device and transfer tries
the known addresses in order. Linked and manually added devices are saved
locally and restored after restarting DropAir. Only linked devices are
available as transfer targets; each linked row has a **Test** action. The
compact Shelf adds pasted text immediately through its single-line paste bar;
typed text can be added with Enter. Each item's send icon reveals its own
linked-device selector. The top includes blank space for moving the window,
the middle scrolls through queued items, and the bottom shows queue and status.
On macOS, text drops also read the native drag pasteboard when the WebView
reports no file paths.

macOS file drag-out uses a native copy drag session with a file URL, which
keeps external applications such as VS Code compatible with Shelf items.

The device name is read from the operating system (Windows hostname or macOS
Computer Name) and can be changed from Settings. This name is the one shown to
other DropAir devices.

The Shelf supports individual item sending and a multi-select batch mode. The
sidebar keeps **Sent** and **Received** transfer history. Discovery sends to the
directed broadcast address of every local IPv4 interface as well as the normal
LAN broadcast, which also covers virtual adapters such as ZeroTier when the
adapter permits broadcast traffic.

Limitations of this preview:

- Transfers are unencrypted and unauthenticated; use it only on trusted LANs.
- Directory and "other" Shelf items are skipped; files and text are supported.
- The transfer is acknowledged after the receiver has written all items
  successfully; incomplete files are removed instead of being added to the
  Shelf.
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

The same page also provides **Run self-check**. It checks the UDP discovery
listener, TCP transfer listener, local TCP loopback connectivity, write access to
the `received` directory, and whether another DropAir instance has been
discovered. Recent listener and transfer errors are retained in the diagnostic
result and are also reported in the application status area.
