# Native plugin inventory

All names, code and sounds are original. Parameters persist by stable key; automation is PLANNED for all.

## Instruments
| Name | Role / FL reference | Techniques | Tests | Status |
|---|---|---|---|---|
| **Trident** | 3-oscillator starter synth (role of 3x OSC) | 3 PolyBLEP oscillators, TPT low-pass, ADSR, 16 voices with stealing | 8 | TESTED |
| **Sampler** | Sample channel (role of Sampler/Audio clip channel) | WAV (int/float, mono/stereo), root key + key tracking, one-shot/gated, reverse, start offset, tune, ADSR, pitch by playback rate | yes | TESTED |
| **Kickr** | Kick drum synth (role of Kicker) | pitch-swept sine, click, drive | yes | TESTED |
| **Clapper** | Clap / snare synth (role of FPC-style drum hits) | body tone + filtered noise, 1-4 clap bursts or snare mode, snappy | yes | TESTED |
| **Hatz** | Hi-hat synth | inharmonic square-wave bank + noise, tone filter, metal amount, choke | yes | TESTED |
| **Orbit** | 2-operator FM synth (role of a small FM synth) | ratio, index + envelope, feedback, ADSR | yes | TESTED |
| **Harpa** | Plucked string (role of a pluck synth) | Karplus-Strong, brightness, pick hardness, sustain | yes | TESTED |
| **Tide** | Wavetable synth | 8 morphing frames band-limited per octave, position LFO, 1-5 unison voices, filter, ADSR | tuning, timbre, aliasing bounds | TESTED |
| **Padz** | 16-pad drum sampler (role of FPC) | sample, volume, pan, tune per pad; C5 and GM key ranges; bundled default kit | pad mapping | TESTED |
| **Chopper** | Loop slicer (role of Slicex) | equal or hit-detected slices on consecutive keys, edge fades, gated or whole-slice | slicing, hit detection | TESTED |

## Effects (mixer inserts)
| Name | Role / FL reference | Techniques | Tests | Status |
|---|---|---|---|---|
| **EQ** | Parametric EQ (role of Parametric EQ 2) | 5 RBJ biquad bands: low cut, low shelf, mid peak, high shelf, high cut | band accuracy | TESTED |
| **Filter** | Resonant filter for sweeps | TPT SVF LP/HP/BP, dry/wet | yes | TESTED |
| **Compressor** | Compressor (role of Fruity Compressor) | soft knee, attack/release, makeup, mix | gain curve | TESTED |
| **Limiter** | Peak limiter (role of Fruity Limiter) | input gain, ceiling, release, safety clip | yes | TESTED |
| **Delay** | Stereo/ping-pong delay (role of Fruity Delay 3) | tempo sync, feedback tone filter | echo timing | TESTED |
| **Reverb** | Algorithmic reverb (role of Fruity Reeverb 2) | 8 damped combs + 4 allpasses per channel, pre-delay, width | decay | TESTED |
| **Chorus** | Stereo chorus | 2 LFO-modulated taps in quadrature | stability | TESTED |
| **Distortion** | Distortion (role of Fruity Blood Overdrive / Fast Dist) | soft/hard clip, wavefold, bitcrush, tone, mix | bounds | TESTED |
| **Multiband Compressor** | 3-band compressor (role of Maximus-style band dynamics) | LR4 crossovers, per-band threshold and gain | flat band sum, band isolation | TESTED |
| **Gate** | Noise gate | threshold, attack, hold, release, range | gating | TESTED |
| **Phaser** | Phaser | 4/6/8 first-order allpass stages, LFO, feedback | audible + dry at zero mix | TESTED |
| **Flanger** | Flanger | modulated short delay with +/- feedback | audible + dry at zero mix | TESTED |
| **Polish** | One-knob enhancer (role of Soundgoodizer) | band compression, saturation, air shelf | dry at zero mix, denser when on | TESTED |
| **Utility** | Gain / pan / width (role of Fruity Stereo Shaper basics) | M/S width, balance pan | identity, mono | TESTED |

Every effect is also checked for silence-in/silence-out and finite output at extreme settings.

Presets: every native instrument and effect has a presets bar (built-in starting points plus user presets in %APPDATA%\Soundesti\Presets).

Planned next: vocoder (needs sidechain routing), pitch correction, spectrum analyzer, granular sampler.