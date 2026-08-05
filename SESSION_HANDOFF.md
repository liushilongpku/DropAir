# Session Handoff

Last verified: 2026-08-05 against local `main` at `0f4778e`; branch is synchronized with `origin/main`.

## Summary

- DropAir is a functional macOS and Windows temporary Shelf for files, folders, and selected text.
- The native Shelf, shake trigger, cross-Space/full-screen behavior, drag-in/out, tray lifecycle, autostart, persisted settings, persisted items, scrolling, item actions, and global shortcut are implemented.
- The Windows implementation adds a Shelf window, tray/shortcut lifecycle, Windows open/reveal commands, platform capability handling, URI-based file drag-out, and an NSIS/MSI build workflow.
- GitHub Windows Smoke Build #6 completed successfully after configuring bundle icons; the Windows installer artifact is ready for manual installation testing.
- Network discovery, pairing, transfer, authentication, encryption, WAN transport, and non-macOS clients remain unimplemented.

## Evidence

- Current source: commit `3bbc3fc` on `main`.
- Latest cloud build: [macOS Smoke Build #25, run 30446415364](https://github.com/liushilongpku/DropAir/actions/runs/30446415364), successful; artifact ID `8721665189`.
- Latest broad feature build: [macOS Smoke Build #24, run 30439463620](https://github.com/liushilongpku/DropAir/actions/runs/30439463620), successful; artifact ID `8718896378`.
- User-confirmed native text dragging: implemented by commits `26edf6a` and `185ec32`; build [run 30437852090](https://github.com/liushilongpku/DropAir/actions/runs/30437852090).
- Reusable validation and Mac regression procedure: `RUNBOOK.md`.
- Stable implementation constraints and architecture: `PROJECT_CONTEXT.md`.
- Current local frontend verification: `npm run build` passed on 2026-08-05.
- Windows build verification: [Windows Smoke Build #6, run 30997498602](https://github.com/liushilongpku/DropAir/actions/runs/30997498602), successful; artifact `DropAir-windows-installers` (ID `8926945059`).
- WSL verification: Windows interop and mounted Windows drives are available; Windows `node`, `npm`, `git`, `winget`, and `choco` are present, but Windows `cargo`, `rustc`, `rustup`, and MSVC `cl.exe` are missing.

## Remaining Risks

- `3bbc3fc` is cloud-build verified but compact-Shelf directory double-click still needs Mac user validation.
- Windows runtime interaction testing is still pending; the WSL host can invoke Windows tools, but the Windows Rust/MSVC toolchain is not installed for local Tauri builds.
- AppKit behavior cannot be executed in the Linux development environment; cross-target checks are compilation evidence only.
- The app is ad-hoc signed and not notarized, so macOS quarantine handling is still required.
- Global shortcut registration failure is only written to stderr; the UI does not report conflicts.
- Persisted Shelf text is stored as plaintext. Persisted file paths may become stale and currently receive errors only when acted on.
- Native drag-out supports existing files and text; directory drag-out is not implemented.
- No transfer security or networking exists yet despite the longer-term product goal.

## Next Actions

1. Download and install the NSIS artifact; execute the Windows regression checklist in `RUNBOOK.md`.
2. Install the MSI artifact and verify install/uninstall behavior plus retained user data.
3. Install build #25 on macOS and verify directory double-click in the compact Shelf, including that rapid delete clicks do not open the directory.
4. Implement invalid-path presentation and removal/relink behavior for restored Shelf items.
5. Before network code, define the device identity, pairing, authentication, encryption, discovery, and transfer protocol boundaries for LAN and ZeroTier environments.
