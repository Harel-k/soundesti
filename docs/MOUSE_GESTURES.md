# Mouse gestures

From the FL manual shortcut page (mouse rows) plus FL conventions. "Verified" = matches the
official manual text; otherwise UNVERIFIED until checked against FL documentation/behaviour.

| Window | Gesture | FL behaviour | Soundesti | Source |
|---|---|---|---|---|
| Piano roll | Left-click empty (Draw) | add note | IMPLEMENTED | manual |
| Piano roll | Right-click note | delete | IMPLEMENTED | manual "Right-Click: Delete selected note(s)" |
| Piano roll | Double-click note | note properties | PLANNED | manual |
| Piano roll | Shift+Left-click | clone selection | IMPLEMENTED | manual |
| Piano roll | Ctrl+Left-click/drag | select | IMPLEMENTED | manual |
| Piano roll | Ctrl+Shift+Left-click | add to selection | IMPLEMENTED (Shift during Ctrl-drag) | manual |
| Piano roll | Alt (held) | bypass snap | IMPLEMENTED | manual |
| Piano roll | Middle-drag | pan view | IMPLEMENTED | manual |
| Piano roll | Left-Shift+Right-drag | pan view | PLANNED (KI-002) | manual |
| Piano roll | Right-Shift+Left-click | slice notes | BLOCKED (KI-002) | manual |
| Piano roll | Right-Alt+Right-click | quantize selected | BLOCKED (KI-002) | manual |
| Piano roll | Left-Alt+Right-click | audio preview | PLANNED | manual |
| Piano roll | Ctrl+Right-click | zoom on selection | PLANNED | manual |
| Piano roll | Alt+wheel | change selected note property | PLANNED | manual |
| Piano roll | Shift+Alt+wheel on notes | nudge notes | PLANNED | manual |
| Piano roll | Wheel / Shift+wheel / Ctrl+wheel | scroll / h-scroll / zoom | IMPLEMENTED | UNVERIFIED (KI-006) |
| Piano roll | Drag note right edge | resize | IMPLEMENTED | FL convention |
| Piano roll | Velocity lane drag | set velocity | IMPLEMENTED | FL convention |
| Channel Rack | Left-click / drag steps | toggle / paint on | IMPLEMENTED | FL convention |
| Channel Rack | Right-click / drag steps | erase | IMPLEMENTED | FL convention |
| Channel Rack | Shift+wheel | move selected channel | PLANNED | manual |
| Channel Rack | Shift+wheel on title bar | change pattern | PLANNED | manual |
| Mixer | Ctrl+Shift+Left-click | multi-select tracks | PLANNED | manual |
| Mixer | Shift+wheel | move track | PLANNED | manual |
| Knobs/faders | Drag vertical, Ctrl fine, double-click reset, wheel | value edit | IMPLEMENTED | UNVERIFIED vs FL defaults |
| Playlist | all playlist gestures | — | PLANNED (Phase 3) | manual |
| Browser | Shift+Up/Down preview, Ctrl+Alt+click replace | — | PLANNED | manual |

Gesture engine: explicit per-editor state machine (`piano_roll.rs` `Drag`); one undo step
per gesture; Escape cancels and restores; release outside the window still ends the gesture.
Not yet tested automatically (needs a UI interaction test harness — TEST_PLAN.md).
