# Soundesti — instructions for Claude Code (and other contributors)

Soundesti is a free, offline, Rust-native DAW with FL Studio-compatible workflow
(shortcuts, windows, gestures) and **original** code, art, names and presets.
Optional private notes live in `CLAUDE.local.md` (gitignored). The repo is public:
never commit personal paths, hardware details, names, or private documents.

## Start of every session
1. Read `docs/PROGRESS.md` (current phase, next priority) and `docs/KNOWN_ISSUES.md`.
2. Check `git status` / `git log` — trust the repo over earlier chat.
3. Continue the next unblocked task; update docs in the same pass.

## Build & test (Windows)
- Toolchain is pinned in `rust-toolchain.toml` (MSVC). Cargo lives in `%USERPROFILE%\.cargo\bin`.
- Some antivirus products randomly block `rustc.exe`/`link.exe`/test binaries
  ("Access is denied. (os error 5)", KI-001). The wrappers retry only those launch failures:
  - `scripts\cargo.ps1 <args>` — cargo with launch-failure retries.
  - `scripts\test.ps1` — builds tests once, runs each binary with retries. `scripts\test.ps1 --ignored` adds the real-audio-device and live-update-feed tests.
- Run: `target\debug\soundesti.exe`. UI checks: `scripts\screenshot.ps1 out.png`, `scripts\input.ps1 "click:x,y" "key:SPACE"` (window-relative physical pixels).
- After editing `assets/shortcuts/*.toml`, run `scripts\gen-shortcut-matrix.ps1`.
- Release: bump `version` in `Cargo.toml`, commit, then `scripts\release.ps1 -Notes "…"` (builds the Installer Wizard, uploads it + `update.json` to GitHub `Harel-k/soundesti`; installed copies auto-update from it).

## Rules
- Audio thread (`Engine::process` and everything it calls): no allocation, locks, I/O, logging. Send heap objects in via `Command`, free them via `Garbage`.
- The project model (`soundesti-project`) is the source of truth; `soundesti-session` diffs it into engine commands. Never let UI state drive audio directly.
- Every edit gets one undo step: `App::checkpoint` before a click edit, or lazy `checkpoint_from(before)` at the end of a drag.
- Shortcuts live only in `assets/shortcuts/fl-studio-workflow.toml` (FL manual transcription). Never add non-FL bindings to that profile.
- Status words: UNRESEARCHED, SPECIFIED, PLANNED, IMPLEMENTING, IMPLEMENTED, TESTED, BLOCKED, NOT_APPLICABLE. TESTED needs an automated test or a recorded manual check.
- Incomplete features may be visible only if clearly labelled (see placeholder windows).
- No copying of FL code, presets, samples, art or names for instruments/effects.
- Commit only verified work.
