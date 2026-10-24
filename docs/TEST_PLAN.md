# Test plan

Run everything: `scripts\test.ps1` (98 tests today). Real device: `scripts\test.ps1 --ignored`.

| Layer | Where | Today |
|---|---|---|
| Unit | every crate | yes |
| Property-based (proptest) | timeline tempo map | yes |
| DSP reference | oscillator frequency, filter -3 dB/attenuation, ADSR shape, stability | yes |
| Audio render | engine: sample-accurate onset, block-size invariance (bit-exact), loops, metronome, mute ramps, panic, routing | yes |
| Project serialization | round trip, atomic failure safety, newer-format rejection, unknown fields, validation | yes |
| Export | length, content, bit-exact float WAV vs engine, cancel, clip safety | yes |
| Shortcut dispatch | generated from the TOML profile: every row resolves in context; mandatory mappings; context overrides; text-field and typing-keyboard suppression | yes |
| UI interaction | `scripts\input.ps1` + `scripts\screenshot.ps1` manual smoke test (recorded in PROGRESS.md) | manual |
| Mouse-gesture automation | needs egui test harness (egui_kittest) | PLANNED |
| MIDI timing, recording loopback, plugin host | — | PLANNED with features |
| Performance benchmarks (criterion) | engine voices, piano roll with 10k notes | PLANNED (next) |

## Manual acceptance checklist (Phase 1–2)
1. Launch → Channel Rack + Piano roll visible, hint bar shows output device.
2. Click steps 1/5/9/13 → Space → hear the pattern loop, playhead moves.
3. Draw notes in Piano roll → they play; Ctrl+Z removes the last edit.
4. F9 → move Trident's fader/mute → level changes audibly, meters move.
5. Ctrl+S → save → reopen → identical pattern plays.
6. Ctrl+R → Render → WAV plays back identically in another player.
Last run: steps 1–3 PASS (2026-10-24); 4–6 covered by automated tests, manual run pending.
