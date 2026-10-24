# Soundesti

A free, offline, Rust-native digital audio workstation with an FL Studio-style workflow:
the same window layout, keyboard shortcuts and editing gestures experienced FL users know,
built from scratch with original code, instruments and artwork.

> Status: **early development (Phases 1–4 in progress)**. See [docs/PROGRESS.md](docs/PROGRESS.md) for
> exactly what works today and what does not.

## What works today
- Real-time audio engine (WASAPI via CPAL) with sample-accurate note scheduling, looping, metronome, panic.
- Channel Rack step sequencer, Piano roll (draw/paint/delete/mute/slice/select/zoom tools, velocity lane, undo).
- **Playlist** with pattern clips and song mode (PAT/SONG), FL-style tools and gestures.
- **Mixer**: master + 125 inserts with faders, pan, mute, solo, meters and **10 insert effect slots** per track.
- **Instruments** (all original): Sampler, Kickr, Clapper, Hatz, Orbit (FM), Harpa (plucked string), Trident (subtractive).
- **Effects** (all original): EQ, Filter, Compressor, Limiter, Delay, Reverb, Chorus, Distortion, Utility.
- **VST, VST3 and CLAP plugins**: instruments and mixer effects with their own windows (OPTIONS › Manage plugins). With FL Studio installed, *FL Studio VSTi* brings FL's own instruments and effects in.
- **FL Studio project import** (.flp): tempo, channels, patterns, notes and the Playlist come across; FL's own plugins become the closest Soundesti instruments.
- FL-style layout: Browser docked on the left, Playlist filling the workspace.
- **Browser** with click-to-preview and drag-to-Channel-Rack, plus the bundled *Soundesti Essentials* pack of 32 original one-shots.
- Save/open `.soundesti` projects (atomic saves, backups, autosave), WAV export (16/24/32-bit).
- 276 FL Studio shortcuts loaded from a machine-readable profile; 156 implemented so far.
- **AI control:** a built-in MCP server lets Claude and other AI assistants drive every command in the app ([docs/MCP.md](docs/MCP.md)).
- Installer Wizard with repair / update / uninstall, and automatic updates from GitHub Releases.

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
