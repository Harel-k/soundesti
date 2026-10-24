# Master plan

Each phase is done only when its exit
criterion is demonstrated (tests + recorded manual check).

| Phase | Scope | Exit criterion | Status |
|---|---|---|---|
| 0 | Research, inventories, architecture, workspace, build/test infra | Cargo build + tests work | DONE |
| 1 | Audio output, transport, tempo, master, test synth, scheduling, basic UI, device selector | Play produces controlled music from scheduled notes | DONE (verified audibly 2026-10-24) |
| 2 | Channel Rack, patterns, step sequencer, Piano roll, shortcuts, velocity, persistence | Compose, save, reopen, replay a pattern | MOSTLY DONE — remaining: pattern clone/delete/colors, per-step params, swing, MIDI import |
| 3 | Playlist: pattern clips, tracks, drag, duplicate, slice, loop, navigation, song mode | Arrange a full song | NEXT |
| 4 | Mixer routing/sends, effect slots, EQ, compressor, delay, reverb, gain staging, automation foundation | Mix multiple instruments with working effects | PLANNED |
| 5 | Recording, audio clips, monitoring, FLAC/MP3 export, MIDI devices + import/export | Record vocals with music and export | PLANNED |
| 6 | CLAP + VST3 hosting, scanning (subprocess), picker, GUIs, state, missing-plugin recovery | Third-party plugins work | PLANNED |
| 7 | Native instrument/effect library (see PLUGIN_INVENTORY.md) | Broad projects without paid plugins | PLANNED |
| 8 | Audio editing, time-stretch, pitch, advanced MIDI tools, consolidation, slicing | Complex editing workflows | PLANNED |
| 9 | Full FL workflow audit (shortcuts, gestures, menus, dialogs) | Every documented interaction has PASS/FAIL/PARTIAL/N.A. + evidence | PLANNED |
| 10 | Advanced tools: spectral, pitch correction, stem separation, audio-to-MIDI | Independently verified | PLANNED |
| 11 | Performance, high-DPI, accessibility, polish, large-project stability, crash resilience | RC stable under test matrix | PLANNED |
| 12 | Installer wizard, portable build, auto-update, release notes, manual, examples | Install → make song → export without help | INSTALLER + UPDATER IMPLEMENTED (pulled forward at owner request); release host, code signing, manual pending |

## Installer & auto-update (owner requirement — implemented 2026-10-24)

Build it: `scripts\package.ps1` → `dist\Soundesti Installer Wizard.exe` (+ `SHA256SUMS.txt`).
Crates: `soundesti-installer` (binary `soundesti-setup`), `soundesti-updater`; app: Help ›
Check for updates / Install update / Restart to update, automatic check at startup (opt-out).
Releases: public repo https://github.com/Harel-k/soundesti, published with `scripts\release.ps1`.
Not done yet: all-users (Program Files) install, Ed25519-signed feed; code signing declined for now. Design as built:

**`Soundesti Setup.exe`** — a single Rust/egui wizard executable (crate `soundesti-installer`):
- **Not installed →** setup wizard: Welcome → License (GPL text) → Install location
  (default per-user `%LOCALAPPDATA%\Programs\Soundesti`, no admin; optional all-users
  `Program Files` with UAC) → Components (Start-menu/desktop shortcuts, `.soundesti`
  file association, example projects) → Install (progress) → Finish (launch).
- **Already installed (detected via registry `HKCU/HKLM\…\Uninstall\Soundesti`) →**
  maintenance menu: **Repair** (re-verify and rewrite files), **Force update**
  (download + install latest even if current), **Modify** (components), **Uninstall**
  (remove files, shortcuts, association, registry; optionally keep user projects/settings),
  plus "Open install folder" and version info.
- Registers itself as the uninstaller in "Apps & features"; payload embedded or downloaded.
- Writes an install manifest (file list + SHA-256) so Repair/Uninstall are exact.

**Auto-update (in the app, crate `soundesti-updater`):**
- Checks a release feed (JSON: version, notes, URL, SHA-256, Ed25519 signature) at most
  daily and on demand (Help › Check for updates). Opt-out setting; no telemetry sent.
- Downloads in the background, verifies signature + hash, then offers "Restart to update",
  handing off to the installer in update mode (never mid-session, never with unsaved work).
- Feed hosting: GitHub Releases once a public repo exists (**needs owner decision**).
- Code signing certificate: optional but recommended to avoid SmartScreen warnings (**owner decision**, costs money).
