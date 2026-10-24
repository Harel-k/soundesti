# Audit — bug hunt + FL Studio 1:1 gap hunt (2026-10-24)

Scope: whole workspace (engine, session, project, app UI, shortcuts, installer, updater),
compared against the FL Studio manual's documented workflow (shortcut page, edition
compare page) and FL's standard window layout. "Verified" FL behaviour comes from the
official manual; anything else is marked *to verify*.

## Bugs found and fixed

| # | Area | Bug | Impact | Fix |
|---|---|---|---|---|
| B1 | Installer | Uninstalling from Apps & features scheduled `rmdir /s` on the install folder, deleting **any user files** saved there | Data loss | Deferred cleanup now deletes only the uninstaller exe, then removes the folder only if empty |
| B2 | Installer / updater | During auto-update the installer could start replacing `soundesti.exe` while the app was still closing → "file in use" failure | Failed updates | File replacement retries for up to 30 s while the file is locked |
| B3 | App / updater | "Restart to update" launched the installer *before* the save prompt; cancelling the prompt left the installer racing an open app | Failed update, confusion | Installer launches only after quitting is confirmed (`AfterConfirm::QuitAndUpdate`) |
| B4 | Piano roll | Paint tool advanced by the last note length; after resizing a note very short (snap off) one drag created hundreds of overlapping notes | Junk notes, CPU | Paint advances by at least one snap unit (¼ step without snap) |
| B6 | MCP server (found while building it) | Answering a rejected request before reading its body made Windows reset the connection, so clients saw `connection reset` instead of the 403 | Confusing failures | Body is always read first; connection closed gracefully |
| B7 | MCP stdio bridge (found while building it) | The app launched by the bridge inherited the client's stdin/stdout pipes, so the client never saw the bridge's output end (hang on reconnect) | AI client hangs | Bridge marks its std handles non-inheritable before launching the app |
| B8 | MCP | A UTF-8 byte-order mark at the start of a message caused a parse error | First request fails for some Windows clients | BOM is ignored (bridge and server; tested) |
| B9 | Installer (found during the owner's real uninstall) | The delayed self-delete passed quoted paths through Rust's argument escaping, which cmd.exe misreads, so `Uninstall Soundesti.exe` (and the folder) were left behind | Leftover files after uninstall | Command line passed verbatim (`raw_arg`); verified: empty folder removed, user files kept |
| B5 | Toolbar (FL parity) | Song position used bar:beat:tick; FL shows **B:S:T** (bars:steps:ticks, 24 ticks/step) | Unfamiliar to FL users | New `BarStepTick` (tested), toolbar shows `1:01:00` style |

Checked with no bug found: seek during held notes, channel removal while sounding,
pattern/tempo switches while playing, undo of deleted channels and of instrument
parameters, text-field and typing-keyboard shortcut suppression, atomic save failure paths,
installer repair/upgrade file cleanup, update tamper rejection.

## FL Studio 1:1 gaps (ranked)

### P0 — core workflow an FL user hits in the first minutes
| Gap | FL behaviour | Status |
|---|---|---|
| Playlist + song mode | Arrange pattern/audio/automation clips on tracks; L switches PAT/SONG (verified: shortcut page) | **Done** for pattern clips (tools, move/resize/slice, song mode); audio/automation clips missing |
| Default project content | New project opens with a small drum kit of sample channels (*to verify exact template*) | **Done**: Kick/Clap/Hat/Snare channels with empty steps |
| Sampler channels + Browser | Drag samples from the Browser into the Channel Rack / Playlist (verified: Browser shortcuts exist) | **Done**: Sampler + Browser (preview, double-click, Enter, drag to rack) + bundled original pack; Playlist audio drops missing |
| Mixer routing & effects | 10 effect slots per insert, routing to any insert, sends, sidechain | **Effects done** (10 slots, 9 effects, bypass, reorder); routing/sends/sidechain missing |
| Automation clips | Automate any parameter from the Playlist | Missing |
| Audio recording | R toggles recording (verified) | Missing |
| Plugin hosting | VST3/CLAP (verified: compare page) | **Done**: VST2, VST3 and CLAP instruments and effects with editors and saved settings; no sandbox, automation or multi-out yet |
| FL project files | Open .flp | **Import done** (patterns, notes, channels, Playlist, tempo); automation/audio clips, mixer and FL plugin sounds not imported |

### P1 — editors that exist but are incomplete
| Gap | FL behaviour (verified unless marked) | Status |
|---|---|---|
| Piano roll tools | Arpeggiator Alt+A, Strum Alt+S, Chop Alt+U, Flam Alt+F, LFO Alt+O, Randomize Alt+R, Riff machine Alt+E, Claw Alt+W, Limit Alt+K, Articulate Alt+L, Scale levels Alt+X, Score flipper Alt+Y, Glue Ctrl+G, Quick chop Ctrl+U | Planned (shortcuts bound, commands report "not implemented") |
| Piano roll lanes | Event lane cycles note properties (F / Shift+F): pan, release, fine pitch, mod X/Y | Velocity only (data model already has pan/release/fine pitch) |
| Piano roll gestures | Double-click note → properties; Ctrl+right-click zoom; Right-Shift slice; Alt+wheel edits property | Missing / blocked (KI-002) |
| Chords, scales, ghost-note swapping | Stamp chords, scale highlighting, double-right-click ghost to swap channel | Missing (ghost display exists) |
| Channel Rack | Groups (Alt+G), zip/unzip, swing knob, graph editor, per-channel step count, "+" plugin list, channel settings window | Missing |
| Pattern management | Clone/delete pattern, pattern colours, pattern picker | Partial (new/rename/select) |
| MIDI | Device input, MIDI import Ctrl+M, export Ctrl+Shift+M | Missing |
| Export | Song export, MP3 Ctrl+Shift+R, FLAC, stems | WAV pattern export only |
| Numpad / L-R modifier shortcuts | Pattern select on numpad, Num+/-, typing octave, slice/quantize gestures | Blocked (KI-002) |

### P2 — main window, polish
| Gap | FL behaviour | Status |
|---|---|---|
| Docked default layout | Browser on the left, Playlist behind, Channel Rack floating | **Done**: Browser docked left (resizable), Playlist fills the workspace, floating windows stay inside it, toolbar window buttons |
| Toolbar | Master pitch, oscilloscope/output monitor, memory meter, record/countdown buttons, snap selector by panel | Partial (master volume added; CPU, BPM, B:S:T, snap) |
| Undo history window | Edit menu history list | Missing (history exists internally) |
| Detached windows, workspaces, multi-monitor | Window layouts | Missing |
| Hint bar behaviour | Shows hovered control help | Shows status + focus only |

### Confirmed matches (spot-checked)
Space play/stop, Ctrl+Space pause, Ctrl+M metronome, Ctrl+H panic, F5–F12 window keys,
F2/F4 pattern naming, Ctrl+Z toggle undo / Ctrl+Alt+Z step undo, piano-roll tool letters
(P B C D E T Z Y), Ctrl+B duplicate, Ctrl+L quick legato, Ctrl+Up/Down octave, right-click
delete, Shift+drag clone, Alt bypass snap, middle-drag pan, Channel Rack mute LED, 1–0 mute
and Ctrl+1–0 solo, Ctrl+L route to free mixer tracks, steps-are-notes with the mini piano-roll
preview, FL note naming (MIDI 60 = C5), 130 BPM default, 125 inserts + master.

## Recommended order to reach practical 1:1
1. ~~Playlist + song mode + pattern clips~~ (done)
2. ~~Sampler channel + Browser + default drum kit~~ (done)
3. ~~Mixer effect slots + first effects~~ (done); routing and sends next
4. Automation clips
5. Piano roll tool suite + event lanes
6. Recording, MIDI devices/import/export
7. CLAP/VST3 hosting
