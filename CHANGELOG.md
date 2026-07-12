# Changelog

## v1.6.0 — 2026-07-12

Initial Windows support.

### Added

- **Windows build** — koi.exe with `+crt-static` runs on stock Windows 10/11 without a VC++ redistributable install (`bundle/windows/build-windows.ps1`, `make windows`, `make windows-zip`).
- Windows CI matrix (`macos-latest` + `windows-latest`) as required gates on `main`.
- Bundled IBM Plex Mono fonts (OFL 1.1) registered process-scoped at startup so the font is available without user install (macOS + Linux paths). Windows path currently falls back to Consolas — see `Known issues`.
- Cross-platform terminal bell: `NSBeep` on macOS, `MessageBeep(MB_OK)` on Windows, `request_user_attention` on Linux.
- ConPTY smoke test (`tests/pty_smoke.rs`) proves headless shell-spawn on both macOS and Windows.
- `PerMonitorV2` DPI awareness on Windows.

### Fixed

- **Main-thread hang on pane close with tmux** — `Pane::drop` now sends `SIGHUP` to the shell's process group (not just the bare PID) before joining the PTY thread, so tmux receives the signal and shell's `wait4` returns promptly. Repro was: Cmd+Shift+D → tmux → Cmd+W → 34-second freeze.
- **Windows blank-window rendering** — `get_glyph` was building `GlyphKey` with `Size::new(0.)`, which crossfont clamps to 1pt internally. On DirectWrite this rasterized every glyph at ~1.33 DIP em size (2×1 pixel bitmaps). Fixed by storing the real `Size` on `GlyphCache` and passing it to every lookup. macOS was hiding the bug because CoreText's crossfont backend ignores `glyph.size` and uses the size bound at `load_font` time.
- **Windows fallback font panic** — hardcoded `"Menlo"` fallback in `glyph_cache.rs` panicked on any non-macOS platform. Now platform-conditional: `Menlo` on macOS, `Consolas` on Windows, `DejaVu Sans Mono` on other Unix.
- Cross-platform bell import: `MessageBeep` lives in `Win32::System::Diagnostics::Debug`, not `Win32::UI::WindowsAndMessaging`.

### Changed

- Release builds now link as the `windows` subsystem, so double-click launch on Windows no longer opens a phantom console window. Log output still streams to the parent console when launched from `cmd`/`powershell` (via `AttachConsole(ATTACH_PARENT_PROCESS)`).
- Branch protection on `main`: PR required, both CI legs (`check (macos-latest)` and `check (windows-latest)`) must pass, linear history, no force-push, no deletion.

### Known issues (Windows)

Cosmetic / feature gaps, not correctness bugs. Filed as separate issues.

- IBM Plex Mono bundled but not visible to DirectWrite: registered fonts via `AddFontResourceExW+FR_PRIVATE` land in GDI's private table, but DirectWrite's shared system font collection is snapshotted at factory creation and doesn't refresh. Real fix requires `IDWriteInMemoryFontFileLoader` + custom `IDWriteFontCollection`, which crossfont doesn't accept via its public API. Falls back to Consolas.
- Terminal mouse click events not forwarded to inner apps.
- Screenshot feature not ported.
- Windows executable has no embedded icon (uses Rust default).

## v1.5.0 — 2026-04-10

Font rendering overhaul, scroll-to-history default, about overlay.

## Earlier

See git log.
