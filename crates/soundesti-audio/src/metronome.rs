/// Short decaying sine click. Accent (bar start) is higher-pitched.
pub(crate) struct Metronome {
    sample_rate: f32,
    triggers: [(u32, bool); 16],
    count: usize,
    phase: f32,
    inc: f32,
    env: f32,
    decay: f32,
}

impl Metronome {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            triggers: [(0, false); 16],
            count: 0,
            phase: 0.0,
            inc: 0.0,
            env: 0.0,
            // ~25 ms to -60 dB.
            decay: (-6.9 / (0.025 * sample_rate)).exp(),
        }
    }

    pub fn trigger(&mut self, offset: u32, accent: bool) {
        if self.count < self.triggers.len() {
            self.triggers[self.count] = (offset, accent);
            self.count += 1;
        }
    }

    /// Adds the click into `left`/`right` and clears pending triggers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32], gain: f32) {
        let mut next = 0;
        let n = left.len();
        for i in 0..n {
            while next < self.count && self.triggers[next].0 as usize <= i {
                let accent = self.triggers[next].1;
                self.inc = if accent { 1568.0 } else { 1046.5 } / self.sample_rate;
                self.phase = 0.0;
                self.env = 1.0;
                next += 1;
            }
            if self.env > 1e-4 {
                let y = (self.phase * std::f32::consts::TAU).sin() * self.env * gain;
                self.phase = (self.phase + self.inc).fract();
                self.env *= self.decay;
                left[i] += y;
                right[i] += y;
            }
        }
        self.count = 0;
    }
}
