# Progress

_Last updated: 2026-10-24 (session 1)._

## Current phase
**Phase 1 (playable core) — done. Phase 2 (sequencer + piano roll + persistence) — mostly done.**
Next: finish Phase 2 gaps, then Phase 3 (Playlist / song mode).

## Environment set up this session
- Installed Rust 1.99.1 (rustup, MSVC), VS 2022 C++ tools 14.43 + Windows SDK 10.0.22621.
- Reference dev machine: Windows 11, integrated GPU (wgpu: Vulkan/DX12), onboard audio, high-DPI display.

## Completed (with evidence)
| Feature | Evidence |
|---|---|
| Musical time (PPQ 960, tempo map, BBT, snapping) | 9 tests incl. proptest round-trips, 10k-bar drift test |
| DSP: PolyBLEP oscillators, ADSR, TPT SVF, smoothing, pan law | 18 tests (frequency, -3 dB cutoff, stability, click-free) |
| Trident synth (16 voices, stealing, panic) | 8 tests (pitch accuracy, offsets, releases, overlapping notes) |
| Real-time engine: lock-free commands, garbage return, sample-accurate scheduling, loop, metronome, mixer routing/meters, tempo changes keep position | 14 tests incl. bit-identical output across block sizes |
| Real audio output (CPAL/WASAPI), silent fallback clock, device errors | null-clock test; `--ignored` real-device test passed on the onboard audio device |
| Project model, steps-are-notes, `.soundesti` atomic save/load, backups, migrations hook, validation | 16 tests |
| Undo/redo with FL Ctrl+Z toggle semantics | 4 tests + UI check |
| Session sync (project → engine diffs, solo, params) | 6 tests |
| WAV export 16/24/32f, cancel, tail, exact match with engine | 6 tests |
| Shortcut system (registry, contexts, conflicts, text/typing suppression, export/import, search) | 14 tests; 276-row FL profile |
| Desktop app: toolbar, Channel Rack, Piano roll, Mixer, instrument editor, audio settings, shortcut search, export dialog, project info, autosave, recent files | Manual UI smoke test 2026-10-24: steps + drawn notes played audibly through the speakers, playhead moved, Space stop/rewind, Ctrl+Z undid last note |

| Renamed project Soundora → **Soundesti** (crates, binary, `.soundesti` format, docs) | builds + all tests |
| **Installer Wizard** (`dist\Soundesti Installer Wizard.exe`, `scripts\package.ps1`): wizard install, maintenance mode (Repair, Force update/reinstall, online update, Modify, Uninstall), Apps & features entry, shortcuts, `.soundesti` association, manifest-based repair/uninstall, silent CLI modes | 5 installer tests; real run 2026-10-24: silent install → verify → damage → repair → GUI uninstall (all traces removed) → GUI fresh install → app launched |
| **Auto-updater** (feed check, verified download, hand-off to installer `--update`; Help menu + startup check, opt-out) | 4 tests incl. real local HTTP download + tamper rejection; no release server yet (KI-007) |

| App icon (music note in a crystal), embedded in both exes, window/taskbar, shortcuts | rendered and reviewed at 16–256 px |

| Bug hunt + FL 1:1 gap audit (docs/AUDIT.md): 6 bugs fixed (installer data loss, update races, paint flood, MCP socket reset, B:S:T clock), TOOLS menu, toolbar master volume | tests + manual checks |
| **Built-in MCP server** (docs/MCP.md): 23 tools, all 193 commands, screenshots, local-only, off by default | 5 unit tests + live AI-style session 2026-10-24 |

**Automated tests: 113 passing** (`scripts\test.ps1`), plus 1 ignored real-device test (passing).

## Not done / known gaps (honest list)
- Playlist / song mode, pattern clips, arrangement (Phase 3) — placeholder window only.
- Recording, audio clips, MIDI devices, MIDI import/export (Phase 5).
- Effects, sends, insert-to-insert routing, automation (Phase 4).
- CLAP/VST3 hosting (Phase 6). Browser. FLAC/MP3 export.
- Numpad and left/right-modifier shortcuts blocked by egui input (KI-002).
- Piano roll: no chord/scale tools, arpeggiator, strum, chop, LFO, randomizer yet; no per-note pan/fine-pitch lanes.
- Not yet measured: UI frame time with thousands of notes, long-session stability, real latency.
- Installer and auto-updater: planned (see MASTER_PLAN.md, Phase 12 track).

## Owner decisions pending
- (Resolved 2026-10-24: license GPL-3.0 confirmed; releases on GitHub Harel-k/soundesti; no code-signing certificate for now.)
- Optional: antivirus exclusion for the repo `target` folder and `%USERPROFILE%\.cargo` / `.rustup` (KI-001).

## Next priorities (owner paused FL-parity work on 2026-10-24 after the icon; resume here)
1. Phase 3: Playlist with pattern clips, song mode transport, time markers, playlist tools/shortcuts.
2. Raw keyboard hook for numpad + left/right modifiers (unblocks 17 shortcuts).
3. Piano roll performance benchmark (thousands of notes), viewport culling check.
4. Phase 4 groundwork: effect trait, insert slots, parametric EQ/compressor/delay/reverb.
