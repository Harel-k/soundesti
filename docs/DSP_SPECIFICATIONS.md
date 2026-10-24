# DSP specifications

| Component | Spec | Verified by |
|---|---|---|
| Oscillator | phase accumulator, inc clamped < 0.49·fs; saw/square PolyBLEP; triangle naive; xorshift noise | frequency ±1 Hz over 1 s; bounded output; no DC on saw |
| ADSR | linear segments; release from current level; min segment 0.5 ms; kill = 2 ms fade | cycle test; no jump on early release; 0-time click-free |
| SVF | Simper/Cytomic TPT, LP/HP/BP, cutoff clamped 10 Hz..0.49·fs, Q ≥ 0.5 | -3 dB at cutoff (Q 0.707); ≤ -20 dB two octaves up; finite under modulation |
| Smoothing | linear ramp, 10 ms for gains | monotonic; exact landing |
| Pan law | balance: centre unity, opposite side attenuated linearly | unit test (FL parity unverified) |
| Engine | f32, max block 512, FTZ/DAZ, NaN→0 on master | bit-identical across block sizes |
| Metronome | 25 ms decaying sine, 1568 Hz accent / 1046.5 Hz beat, gain 0.35 | click-position test |
| Export dither | TPDF ±1 LSB on 16-bit only | clip test |
