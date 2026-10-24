# Soundesti

A free, offline, Rust-native digital audio workstation with an FL Studio-style workflow:
the same window layout, keyboard shortcuts and editing gestures experienced FL users know,
built from scratch with original code, instruments and artwork.

> Status: **early development (Phase 1–2)**. See [docs/PROGRESS.md](docs/PROGRESS.md) for
> exactly what works today and what does not.

## What works today
- Real-time audio engine (WASAPI via CPAL) with sample-accurate note scheduling, looping, metronome, panic.
- Channel Rack step sequencer, Piano roll (draw/paint/delete/mute/slice/select/zoom tools, velocity lane, undo), Mixer (master + 125 inserts with faders, pan, mute, solo, meters).
- **Trident** — an original 3-oscillator subtractive synth.
- Save/open `.soundesti` projects (atomic saves, backups, autosave), WAV export (16/24/32-bit).
- 276 FL Studio shortcuts loaded from a machine-readable profile; 147 implemented so far.

## Build
Windows 11, Rust (pinned in `rust-toolchain.toml`), Visual Studio C++ build tools + Windows SDK.

```bash
cargo run -p soundesti-app --release
```

On machines where antivirus interferes with builds, use `scripts\cargo.ps1` and `scripts\test.ps1`.

## License
GPL-3.0-or-later ([LICENSE](LICENSE)). Dependency licenses: [docs/LICENSE_AUDIT.md](docs/LICENSE_AUDIT.md).
Soundesti is an independent project and is not affiliated with or endorsed by Image-Line.
"FL Studio" is a trademark of Image-Line Software.
