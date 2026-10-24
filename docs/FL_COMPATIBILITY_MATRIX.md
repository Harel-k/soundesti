# FL workflow compatibility matrix

Status words per CLAUDE.md. Evidence: T = automated test, M = manual UI check (date), — = none yet.
Shortcut-level detail: SHORTCUT_MATRIX.md. Gesture detail: MOUSE_GESTURES.md.

## Transport & global
| Behaviour | Status | Evidence | Notes |
|---|---|---|---|
| Space play/stop, stop rewinds to pattern start | TESTED | T engine, M 2026-10-24 | |
| Ctrl+Space play/pause | IMPLEMENTED | — | |
| Pattern mode looping | TESTED | T pattern_loops | |
| Song mode (L) | BLOCKED | — | needs Playlist; command refuses with message |
| BPM 10–522, live tempo change keeps position | TESTED | T tempo tests | FL range per manual is similar; exact limits unverified |
| Metronome (Ctrl+M), accent on bar | TESTED | T metronome | count-in/precount PLANNED |
| Panic (Ctrl+H) | TESTED | T panic | |
| Typing keyboard to piano (Ctrl+T) | IMPLEMENTED | T suppression | layout parity unverified (SM-5) |
| Ctrl+Z toggle undo/redo, Ctrl+Alt+Z step undo | TESTED | T history, M 2026-10-24 | |
| Window toggles F5–F12, Alt+F8, Esc, Tab | IMPLEMENTED | T dispatch | Playlist/Browser/MIDI windows are labelled placeholders |
| Hint bar | IMPLEMENTED | M | shows status + focus context |

## Channel Rack
| Behaviour | Status | Evidence |
|---|---|---|
| Steps toggle on click, left-drag paints, right-drag erases | IMPLEMENTED | M 2026-10-24 (clicks) |
| Steps are notes; mini piano-roll preview when notes aren't steps | TESTED | T steps_are_notes, M |
| Mute LED, solo (right/Ctrl+click), volume/pan knobs, mixer track selector | IMPLEMENTED | T solo |
| Channel select, multi-select, clone, delete, move, rename, route to free tracks | IMPLEMENTED | T clone/remove |
| Copy/cut/paste steps, shift steps | IMPLEMENTED | — |
| Channel groups, zip/unzip, swing, per-step params, step graph | PLANNED | — |

## Piano roll
| Behaviour | Status | Evidence |
|---|---|---|
| Draw: click creates note (last length), drag moves, right edge resizes, Shift+drag clones | IMPLEMENTED | M (create) |
| Right-click deletes (also while dragging) | IMPLEMENTED | — |
| Ctrl+drag select, Select tool, Paint, Delete, Mute, Slice, Zoom tools | IMPLEMENTED | — |
| Alt bypasses snap; snap menu; Backspace toggles snap | IMPLEMENTED | — |
| Velocity lane editing (selected-only when a selection exists) | IMPLEMENTED | — |
| Keyboard preview, ruler click seeks, playhead, ghost channels | IMPLEMENTED | — |
| Escape cancels an in-progress drag (restores state) | IMPLEMENTED | — |
| Quick legato, quick quantize, quantize dialog, duplicate right, transpose, nudge | IMPLEMENTED | — |
| Chords/scales/arp/strum/chop/flam/LFO/randomize/riff machine/claw/limit/articulate | PLANNED | — |
| Note pan/release/fine-pitch lanes, slide/portamento notes | PLANNED | model fields exist |

## Mixer
| Behaviour | Status | Evidence |
|---|---|---|
| Master + 125 inserts, faders, pan, mute, solo, peak meters | TESTED | T routing/meters |
| Channel → insert routing, link (Ctrl+L), select linked, move tracks, rename | IMPLEMENTED | T routing |
| Effect slots, sends, sidechain, PDC, stereo separation, record arm | PLANNED | — |

## Files & export
| Behaviour | Status | Evidence |
|---|---|---|
| Save / Save as / Save new version (Ctrl+N) / Open / recent (Alt+0-9) | TESTED | T save/load/version |
| Atomic save + .bak, autosave every 2 min to %APPDATA%\Soundesti | TESTED (save path) | T |
| Unsaved-changes prompt on new/open/quit | IMPLEMENTED | — |
| WAV export (Ctrl+R) 16/24/32f, loops, tail, cancel | TESTED | T export |
| MP3 (Ctrl+Shift+R) | BLOCKED | licensing review |
| MIDI export/import, FLAC, stems | PLANNED | — |
| `.flp` import | UNRESEARCHED | separate research track |
