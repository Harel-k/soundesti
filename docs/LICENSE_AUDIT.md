# License audit (in progress)

Project license: **GPL-3.0-or-later** (D-008, confirmed by the owner 2026-10-24; full text in LICENSE).

| Dependency | License (crates.io metadata) | GPL-3 compatible | Notes |
|---|---|---|---|
| eframe / egui / epaint / emath 0.37 | MIT OR Apache-2.0 | yes | |
| wgpu (via eframe) | MIT OR Apache-2.0 | yes | |
| cpal 0.18 | Apache-2.0 | yes | ASIO backend needs Steinberg ASIO SDK — separate license, not bundled yet |
| rtrb 0.4 | MIT OR Apache-2.0 | yes | |
| serde / serde_json / toml | MIT OR Apache-2.0 | yes | |
| hound 3.5 | Apache-2.0 | yes | |
| rfd 0.17 | MIT | yes | |
| thiserror, anyhow, log, env_logger | MIT OR Apache-2.0 | yes | |
| proptest (dev) | MIT OR Apache-2.0 | yes | |

To do before the first release: run `cargo deny check licenses` over the full transitive tree
(fonts in egui's default font set are OFL/Apache — verify bundling notices), and review:
- **MP3 encoding:** MP3 patents expired (2017); choose an encoder with a GPL-compatible license (e.g. LAME is LGPL) — pending.
- **VST3 SDK:** Steinberg relicensed the VST3 SDK under MIT in 2025 (verify current terms before Phase 6). VST2: SDK no longer distributed → not supported.
- **CLAP:** MIT — compatible.
- **AI model weights** (stem separation etc.): per-model review.
- No FL Studio assets, presets, samples or code are used.

## Added 2026-10-24
| Crate | License | Use |
|---|---|---|
| flacenc | Apache-2.0 | FLAC export |
| mp3lame-encoder / mp3lame-sys (LAME) | LGPL-3.0 | MP3 export (LGPL is compatible with Soundesti's GPL-3.0) |
| vst3 | MIT OR Apache-2.0 | VST3 hosting bindings |
| clap-sys | MIT OR Apache-2.0 | CLAP hosting bindings |
| libloading | ISC | loading plugin DLLs |
| midir | MIT | MIDI keyboard input |
