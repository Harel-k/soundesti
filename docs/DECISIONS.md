# Decisions

| ID | Decision | Why | Rejected alternatives |
|---|---|---|---|
| D-001 | Rust 1.99.1 stable, MSVC target, edition 2024, pinned in `rust-toolchain.toml` | Reproducible builds; MSVC needed for ASIO/VST3 SDK interop later | GNU toolchain (less standard on Windows) |
| D-002 | UI: egui/eframe 0.37 on wgpu (Vulkan/DX12) with custom-painted editors | Immediate mode fits dense, constantly-updating DAW views; GPU rendering; pure Rust | iced (less suited to custom canvases), Slint (licensing), web/Electron (forbidden by brief) |
| D-003 | Audio I/O: CPAL 0.18 (WASAPI shared) | Maintained, cross-platform, Rust-native; ASIO later via CPAL's optional backend | PortAudio bindings (C dependency) |
| D-004 | Engine/control communication: `rtrb` 0.4 SPSC ring buffers + atomics; heap objects built on the UI thread, freed via a garbage queue | Allocation-free, lock-free audio thread | Mutex-shared state (priority inversion), `basedrop` (extra dependency for one pattern) |
| D-005 | Musical time = integer ticks at PPQ 960; engine keeps an integer sample counter and converts through a stateless tempo map | No floating-point drift; 960 divides triplets/quintuplets/32nds exactly | FL's 96 PPQ default (too coarse), float beats (drift) |
| D-006 | Steps are notes (one data model for Channel Rack and Piano roll) | Matches FL semantics; no sync bugs between two representations | Separate step arrays |
| D-007 | `.soundesti` = versioned JSON (serde), atomic temp-file + rename, `.bak` of previous save | Human-diffable, robust, easy migrations; size is small without audio | Binary format (premature), SQLite (overkill now) |
| D-008 | License: GPL-3.0-or-later (**confirmed by owner 2026-10-24**; full text in LICENSE) | Guarantees "free forever"; all current deps (MIT/Apache/Zlib) are GPL-compatible | MIT/Apache (allows closed forks) |
| D-009 | Undo = whole-project snapshots, one per gesture | Projects contain no audio yet, so snapshots are tiny and always correct; FL Ctrl+Z toggle semantics | Command/diff undo (planned when audio clips arrive) |
| D-010 | Crates are added when they have real content, not as empty placeholders | Brief forbids fake scope; the 35-crate layout is the target, reached incrementally | Creating all 35 empty crates up front |
| D-011 | `soundesti-session` bridges project → engine by diffing mirrors | UI can call `sync()` freely; only real changes reach the audio thread | UI sending engine commands ad hoc |
| D-012 | Shortcut profile is data (TOML) transcribed from the FL manual; statuses in a separate file; matrix doc generated | Single source of truth; regression tests generated from the same file | Hard-coded key handling |
| D-013 | Installer: a Rust-built "Soundesti Setup" wizard exe with maintenance mode (install / repair / force update / uninstall) and a signed update feed used by in-app auto-update | Owner requirement; Rust-first; full control of maintenance UX | Inno Setup / WiX (no Rust, weaker maintenance UX) — revisit if code-signing/MSI needs arise |
| D-014 | Instrument names are original (Trident, …) | Legal clean-room; FL names are trademarks | Reusing FL plugin names |
