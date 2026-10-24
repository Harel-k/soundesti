# Progress

_Last updated: 2026-10-24 (session 2: instruments, playlist, effects, browser)._

## Current phase
**Phases 1-2 done; Phase 3 (Playlist / song mode) and Phase 4 basics (insert effects) implemented.**
Next: automation clips, insert-to-insert routing and sends, piano-roll tool suite.

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
| **Built-in MCP server** (docs/MCP.md): 26 tools (incl. mixer effects, playlist, browser), all 194 commands, screenshots, local-only, off by default | unit tests + live AI-style session 2026-10-24 |
| **Instruments**: Sampler (WAV, root/tune/ADSR/reverse/start), Kickr (kick), Clapper (clap/snare), Hatz (hi-hats), Orbit (2-op FM), Harpa (plucked string), Trident; grouped "+ Add" menu; new projects open with a Kick/Clap/Hat/Snare kit | 17 synthesis tests + sample loading tests |
| **Playlist + song mode** (PAT/SONG, L): pattern clips on 50+ tracks, draw/paint/select/delete/mute/slice/zoom tools, move/resize/clone, copy/paste/duplicate, picker panel, ruler seek | playlist model tests + `song_mode_plays_playlist_clips` |
| **Mixer insert effects**: 10 slots per track (pre-fader, master too), bypass, reorder, effect windows with knobs; 9 original effects: EQ, Filter, Compressor, Limiter, Delay (tempo sync, ping-pong), Reverb, Chorus, Distortion, Utility | 10 effect tests (stability at extreme settings, EQ/compressor/delay accuracy, reverb decay) + engine and session effect tests |
| **Browser** (Alt+F8): folder tree (bundled packs, Music, Downloads, Desktop, Documents, user folders), search, click-to-preview, double-click = new channel, Enter = replace selected channel, Shift+Up/Down preview stepping, drag onto Channel Rack rows | preview test + listing test |
| **Soundesti Essentials** sample pack: 32 original one-shots rendered from Soundesti's own instruments/effects (`cargo run -p soundesti-session --example make_pack`), installed to `Packs\` | renders reproducibly |
| **Vector icons** everywhere (no emoji or font glyphs) | visual check |
| **FL-style docked layout**: Browser docked left (resizable), Playlist fills the workspace, floating windows stay inside it, toolbar window buttons, focused-caption highlight | visual check 2026-10-24 |
| **FL Studio project import** (.flp, FL 9 to FL 2025): tempo, time signature, channels (name, color, volume, pan, mute, insert), patterns + notes (velocity, pan, release, fine pitch), Playlist pattern clips + track names/colors, samples resolved from FL's folders or next to the project; FL plugins mapped to the closest Soundesti instrument; grouped import report | 7 tests (synthetic files, fuzzed truncation); all 46 FL Studio 2026 demo projects import without errors |
| **Third-party plugins: VST2, VST3, CLAP** — plugin manager (OPTIONS > Manage plugins) with crash-isolated scanning and cache; plugins as Channel Rack instruments and mixer effects; their own editor windows; parameters; settings saved in projects; missing plugins reported and silent; FL Studio VSTi brings FL's own instruments/effects in | host tests incl. a CLAP round trip with a bundled test synth; manual checks 2026-10-24 with FL Studio VSTi (VST2), Elgato EQ/Compressor/Noise Removal and Voicemod Mr. Meowington (VST3): scan, sound, editor, save + reopen, remove, quit |
| **AI assistant setup** window (HELP menu): exact steps + Copy + one-click setup for Claude Code, Codex, Cursor, VS Code, Windsurf, Gemini CLI, Claude Desktop; built-in `--mcp-stdio` bridge | 4 unit tests; live bridge test with app auto-launch |

**Automated tests: 152 passing** (`scripts\test.ps1`), plus 1 ignored real-device test (passing).

## Not done / known gaps (honest list)
- Playlist: no audio or automation clips, time markers, performance mode or clip-level transpose yet; song export renders the pattern only.
- Recording, audio clips, MIDI devices, MIDI import/export (Phase 5).
- Mixer: sends, insert-to-insert routing, sidechain, PDC and automation are not implemented; effects have generic knob panels (no custom graphical editors yet).
- .flp import: automation clips, audio clips, mixer settings/effects and FL plugin sounds are not imported (FL's plugin data is private).
- Plugins run inside Soundesti's process (no sandbox yet): a crashing plugin closes Soundesti. No plugin automation, sidechain or multi-output routing yet; plugin settings are not part of undo. FLAC/MP3 export. Browser shows WAV only (no presets, projects or plugin database).
- Numpad and left/right-modifier shortcuts blocked by egui input (KI-002).
- Piano roll: no chord/scale tools, arpeggiator, strum, chop, LFO, randomizer yet; no per-note pan/fine-pitch lanes.
- Not yet measured: UI frame time with thousands of notes, long-session stability, real latency.
- Installer and auto-updater: planned (see MASTER_PLAN.md, Phase 12 track).

## Owner decisions pending
- (Resolved 2026-10-24: license GPL-3.0 confirmed; releases on GitHub Harel-k/soundesti; no code-signing certificate for now.)
- Optional: antivirus exclusion for the repo `target` folder and `%USERPROFILE%\.cargo` / `.rustup` (KI-001).

## Next priorities
1. Automation clips (Playlist) and song-mode WAV export.
2. Mixer sends / insert-to-insert routing / sidechain.
3. Piano roll tool suite (arpeggiator, strum, chop, randomize, scales/chords) and event lanes.
4. Raw keyboard hook for numpad + left/right modifiers (unblocks 17 shortcuts).
5. Audio clips in the Playlist (drag from the Browser), recording.
