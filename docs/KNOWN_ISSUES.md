# Known issues

| ID | Severity | Issue | Status / workaround |
|---|---|---|---|
| KI-001 | Dev env | Some third-party antivirus products intermittently block `rustc.exe`, `link.exe` and new test executables ("Access is denied. (os error 5)"); intercepted test launches occasionally run with a stray filter argument ("0 passed; N filtered out"). | Use `scripts\cargo.ps1` / `scripts\test.ps1` (retry only launch failures). Permanent fix: exclude the repo's `target` folder and `%USERPROFILE%\.cargo` / `.rustup` from real-time scanning. |
| KI-002 | Medium | egui 0.37 key events don't distinguish numpad digits from the main row, or left from right Shift/Alt in modifier state. 17 FL bindings (pattern select, Num+/-, typing octave, song bar jumps) and some mouse gestures (Right-Shift slice, Right-Alt quantize) can't fire. | BLOCKED in the matrix. Fix: read winit raw `KeyCode` (Numpad*, ShiftRight…) via a window event hook. |
| KI-003 | Low | On first launch eframe may restore an oversized window from a previous run on high-DPI displays. | App maximizes on start. |
| KI-004 | Low | Some emoji/glyphs are missing from egui's default font (shown as boxes). | Avoid them in labels; bundle an icon font later (original or OFL-licensed). |
| KI-005 | Low | Editing notes while playing replaces the snapshot; already-sounding notes keep their old length until released. | Acceptable; matches most DAWs. |
| KI-007 | Medium | Update feed (GitHub Releases `update.json`) is trusted via HTTPS + the SHA-256 it lists; no Ed25519 signature yet, so a compromised GitHub account could ship a bad update. | Add an offline signing key + signature check. |
| KI-008 | Low | Installer is per-user only (no Program Files / all-users option) and the exe is unsigned, so Windows SmartScreen may warn on first run ("More info → Run anyway"). | Owner declined a code-signing certificate for now (2026-10-24). |
| KI-006 | Info | Wheel = vertical scroll, Shift+wheel = horizontal, Ctrl+wheel = zoom in the Piano roll. Exact FL wheel behaviour not verified yet. | Track in MOUSE_GESTURES.md. |

## KI-PLUGIN-1: plugins run in-process
Third-party plugins are loaded into Soundesti's own process. A plugin that crashes closes Soundesti (autosave keeps recent work). Scanning is isolated in a child process. FL Studio VSTi crashes in its own DLL shutdown, so Soundesti exits without unloading plugin code once any plugin was loaded.
