//! Stereo / ping-pong delay with tempo sync and a tone filter in the feedback path.

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont, stepped};

pub const TYPE_ID: &str = "soundesti.delay";
const MAX_SECONDS: f32 = 2.5;

pub mod p {
    pub const SYNC: u32 = 0;
    pub const TIME: u32 = 1;
    pub const FEEDBACK: u32 = 2;
    pub const TONE: u32 = 3;
    pub const PINGPONG: u32 = 4;
    pub const MIX: u32 = 5;
}

const SYNC_LABELS: &[&str] = &["Off (ms)", "1/16", "1/8", "1/8 dotted", "1/4", "1/4 dotted", "1/2", "1 bar"];
/// Delay length in beats for each sync choice (index 0 = free time).
const SYNC_BEATS: [f32; 8] = [0.0, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0];

pub static PARAMS: [ParamInfo; 6] = [
    stepped(p::SYNC, "delay.sync", "Tempo sync", "Time", 7.0, 3.0, SYNC_LABELS),
    cont(p::TIME, "delay.time", "Time (free)", "Time", 1.0, 2000.0, 375.0, "ms"),
    cont(p::FEEDBACK, "delay.feedback", "Feedback", "Echo", 0.0, 0.95, 0.4, ""),
    cont(p::TONE, "delay.tone", "Tone", "Echo", 300.0, 20_000.0, 6_000.0, "Hz"),
    stepped(p::PINGPONG, "delay.pingpong", "Ping-pong", "Echo", 1.0, 1.0, &["Off", "On"]),
    cont(p::MIX, "delay.mix", "Mix", "Output", 0.0, 1.0, 0.3, ""),
];

pub struct Delay {
    sr: f32,
    p: Params<6>,
    bpm: f32,
    buf: [Vec<f32>; 2],
    pos: usize,
    lp: [f32; 2],
}

impl Delay {
    pub fn new(sr: f32) -> Self {
        let n = (sr * MAX_SECONDS) as usize + 2;
        Self { sr, p: Params::new(&PARAMS), bpm: 120.0, buf: [vec![0.0; n], vec![0.0; n]], pos: 0, lp: [0.0; 2] }
    }

    pub fn delay_samples(&self) -> usize {
        let sync = self.p.get(p::SYNC) as usize;
        let secs =
            if sync == 0 { self.p.get(p::TIME) / 1000.0 } else { SYNC_BEATS[sync.min(7)] * 60.0 / self.bpm.max(10.0) };
        ((secs * self.sr) as usize).clamp(1, self.buf[0].len() - 1)
    }
}

impl Effect for Delay {
    crate::effect_common!(TYPE_ID, "Delay", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let d = self.delay_samples();
        let n = self.buf[0].len();
        let (fb, mix) = (self.p.get(p::FEEDBACK), self.p.get(p::MIX));
        let ping = self.p.get(p::PINGPONG) > 0.5;
        let a = (std::f32::consts::TAU * self.p.get(p::TONE) / self.sr).min(1.0);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let rd = (self.pos + n - d) % n;
            let (dl, dr) = (self.buf[0][rd], self.buf[1][rd]);
            self.lp[0] += a * (dl - self.lp[0]);
            self.lp[1] += a * (dr - self.lp[1]);
            let (fl, fr) = (self.lp[0] * fb, self.lp[1] * fb);
            if ping {
                // Input enters on the left; each repeat crosses to the other side.
                self.buf[0][self.pos] = (*l + *r) * 0.5 + fr;
                self.buf[1][self.pos] = fl;
            } else {
                self.buf[0][self.pos] = *l + fl;
                self.buf[1][self.pos] = *r + fr;
            }
            self.pos = (self.pos + 1) % n;
            // Dry signal stays at full level; the echoes are added on top.
            *l += dl * mix;
            *r += dr * mix;
        }
    }

    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = bpm;
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
        self.lp = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_arrives_at_the_synced_time() {
        let mut d = Delay::new(48_000.0);
        d.set_tempo(120.0);
        d.set_param(p::SYNC, 4.0); // 1/4 = one beat = 0.5 s at 120 BPM
        d.set_param(p::PINGPONG, 0.0);
        d.set_param(p::TONE, 20_000.0);
        assert_eq!(d.delay_samples(), 24_000);
        let mut l = vec![0.0; 30_000];
        let mut r = vec![0.0; 30_000];
        l[0] = 1.0;
        r[0] = 1.0;
        d.process(&mut l, &mut r);
        let (peak_i, _) = l[1..]
            .iter()
            .enumerate()
            .fold((0, 0.0f32), |(bi, bv), (i, &v)| if v.abs() > bv { (i + 1, v.abs()) } else { (bi, bv) });
        assert!((peak_i as i64 - 24_000).abs() <= 2, "echo at {peak_i}");
    }
}
