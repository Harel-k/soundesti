# Architecture

```
            ┌──────────────── UI thread (eframe/egui, wgpu) ────────────────┐
 keyboard → │ raw_input_hook → Keymap.resolve(context) → App::run(Command)  │
 mouse    → │ editors (Channel Rack, Piano roll, Mixer) edit Project        │
            │ History (undo snapshots)      Project = source of truth       │
            │ Session.sync(project): diff → engine Commands                 │
            └──────────────┬───────────────────────────▲────────────────────┘
               rtrb SPSC   │ Command (boxed instruments,│ Garbage (old snapshots,
               queue       ▼ snapshots, params, notes)  │ instruments to free)
            ┌──────────── audio thread (CPAL/WASAPI callback) ──────────────┐
            │ Engine::process: drain commands → schedule notes (sample-     │
            │ accurate, tempo map) → instruments → channel vol/pan → mixer  │
            │ inserts → master → metronome → device buffer                  │
            │ telemetry via atomics: position, meters, CPU load, overloads  │
            └───────────────────────────────────────────────────────────────┘
 Export: same Engine run offline (soundesti-export) → WAV.
```

## Crates (current)
| Crate | Role |
|---|---|
| `soundesti-timeline` | PPQ ticks, tempo map, time signatures, BBT, snapping |
| `soundesti-dsp` | oscillators (PolyBLEP), ADSR, SVF filter, smoothing, pan, denormals |
| `soundesti-plugin-api` | native `Instrument` trait, note events, parameter metadata |
| `soundesti-synthesis` | native instruments (Trident) + factory |
| `soundesti-audio` | real-time engine, commands, snapshots, mixer, metronome, telemetry |
| `soundesti-audio-device` | CPAL device I/O, fallback clock, device health |
| `soundesti-project` | project model, `.soundesti` format, atomic save, history |
| `soundesti-session` | project → engine synchronization |
| `soundesti-commands` | central command registry (stable IDs) |
| `soundesti-shortcuts` | key parsing, contexts, keymaps, dispatch rules, typing piano |
| `soundesti-export` | offline render + WAV writer |
| `soundesti-app` | desktop application (binary `soundesti`) |

Planned crates (added when they get real code): playlist/timeline arrangement, mixer
routing graph, automation, effects, sampler, midi, recording, media decoding, waveform,
plugin hosts (CLAP/VST3), browser, presets, analysis, installer, updater.

## Real-time rules
See CLAUDE.md. Key techniques: preallocated event vectors with capacity checks,
`sort_unstable` (no allocation), leaking instead of freeing if the garbage queue is ever
full (counted in telemetry), FTZ/DAZ per callback, NaN sanitizing on the master bus,
gain smoothing (10 ms ramps) for click-free mute/volume/pan.
