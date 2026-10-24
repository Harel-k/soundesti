# Feature inventory (reference: FL Studio 2026, All Plugins Edition)

Source: https://www.image-line.com/fl-studio/compare (fetched 2026-10-24). Editions on the
page: Fruity, Producer, Signature Bundle, **All Plugins Edition** (top tier; no prices shown;
lifetime free updates). Counts on the page for All Plugins: 16 core features, 3 audio
editors, 71 effects, 39 instruments, 6 video/visual tools.

Feasibility key (master prompt §1F): **IND** independently reproducible · **OSS** via
legal open-source components · **ALT** functionally equivalent alternative algorithm ·
**EXT** depends on external plugin licenses · **SMP** depends on licensed sample data ·
**SVC** needs a third-party/online service · **N/A** incompatible with our requirements.

## Core features
| FL feature | Feasibility | Soundesti status |
|---|---|---|
| Piano roll, Mixer, Channel Rack, Automation clips, Time-signature changes | IND | Piano roll/Mixer/Rack IMPLEMENTED (partial); automation, TS changes PLANNED |
| Full song arrangement (Playlist) | IND | PLANNED (Phase 3) |
| Audio recording, Audio clips | IND | PLANNED (Phase 5) |
| MIDI support, MIDI out | IND | PLANNED (Phase 5) |
| VST2 / VST3 / AU / CLAP hosting | VST3+CLAP: OSS/IND (license review); VST2: N/A (SDK no longer licensed); AU: macOS-only, later | PLANNED (Phase 6) |
| Stem separation | ALT (open models, license check) | PLANNED (Phase 10) |
| Denoising | ALT | PLANNED |
| Loop Starter (updated 2026) | ALT (generative, offline) | PLANNED |
| Chord Generator (updated 2026) | IND | PLANNED |
| Gopher AI assistant (updated 2026) | SVC (optional; offline alternative required) | PLANNED, optional |
| Audio Logger (new 2026: background capture) | IND | PLANNED |
| Sound content incl. FL Cloud | SMP/SVC — will ship original/CC0 content only | PLANNED |
| FL Studio Mobile Rack + FX | N/A (proprietary product) | NOT_APPLICABLE |
| FL Studio Remote | ALT (own remote protocol) | PLANNED, low priority |
| Envelope / Keyboard / Voltage controllers | IND | PLANNED |
| Audio editors: Edison, Newtone, Newtime | ALT | PLANNED (Phase 8/10) |

## Instruments (39) → Soundesti roles
Original Soundesti names are assigned when each instrument is built; "Trident" exists today.

| Role (FL reference) | Feasibility | Soundesti |
|---|---|---|
| 3-osc subtractive (3x OSC) | IND | **Trident — TESTED** |
| Basic sampler (Channel Sampler), Drumpad, FPC | IND | PLANNED Phase 5/7 |
| Preset-based sound design (FLEX), layered presets | IND + original presets | PLANNED |
| FM/additive/subtractive (Sytrus), FM (DX10) | IND | PLANNED |
| Additive + resynthesis (Harmor), additive w/ subtractive workflow (Harmless), morphing additive (Morphine), metallic additive (Ogun) | ALT | PLANNED |
| Physical modelling plucked/strings (Sakura, Plucked!), drums (Drumaxx) | ALT | PLANNED |
| Analog emulations (Kepler, Kepler Exo, Sawer, Poizone, SimSynth, MiniSynth, Transistor Bass, BooBass) | ALT | PLANNED |
| Hybrid FM-subtractive (Toxic Biohazard), GMS, Autogun (random patches) | ALT | PLANNED |
| Sliced loops (Slicex, Fruity Slicer 1/2), granular (Granulizer), BeepMap, Wave Traveller | IND | PLANNED |
| Drum synths (BassDrum, Fruity Kick, DrumSynth Live) | IND | PLANNED |
| Full sampler (DirectWave Full/Player), SoundFont player | IND (SF2 parsing) | PLANNED |
| Piano (FL Keys) | SMP/ALT (modelled or CC0 samples) | PLANNED |
| Speech synthesizer | ALT (offline TTS, license check) | PLANNED, low |

## Effects (71) → roles
| Role (FL reference) | Feasibility | Status |
|---|---|---|
| EQ (Parametric EQ / EQ2 / EQUO), filters (Filter, Love Philter) | IND | PLANNED Phase 4 |
| Dynamics (Compressor, Limiter, Multiband Comp, Maximus, Emphasis, Soft Clipper, Transient Processor, Transmitter) | IND | PLANNED Phase 4/7 |
| Delay (Delay 2/3, Delay Bank, Multiband Delay), Reverb (Reeverb 2, LuxeVerb), Convolver | IND | PLANNED Phase 4/7 |
| Modulation (Chorus, Flanger, Flangus, Phaser, Vintage Chorus/Phaser, Hyper Chorus) | IND | PLANNED |
| Distortion (Blood Overdrive, Fast Dist, WaveShaper, Squeeze, Distructor, Hardcore) | IND | PLANNED |
| Stereo (Stereo Enhancer/Shaper, Spreader, Balance, PanOMatic) | IND | PLANNED |
| Pitch (Pitcher, Pitch Shifter, Frequency Shifter), Vocoders (Vocoder, Vocodex) | ALT | PLANNED |
| Time/gate (Gross Beat, Transporter, Scratcher) | ALT | PLANNED |
| Enhancers (Soundgoodizer, Low Lifter, Emphasizer), Effector, Tuner | ALT | PLANNED |
| Routing/control (Send, Patcher, Peak/Formula/X-Y controllers, Control Surface, VFX family) | IND | PLANNED |
| Notebooks, HTML notebook, Razer Chroma, LSD | IND / N/A (Razer: vendor SDK) | LOW |

## Visual (6)
Big Clock, dB Meter, Spectroman, Wave Candy: IND (PLANNED); Video Player, ZGameEditor visualizer: ALT, low priority.
