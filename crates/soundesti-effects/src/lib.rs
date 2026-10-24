//! Soundesti native insert effects (original implementations). Each covers the role of a
//! common mixer effect: EQ, filter, compressor, multiband compressor, gate, limiter,
//! delay, reverb, chorus, phaser, flanger, distortion, an enhancer and a utility.

mod base;
pub mod chorus;
pub mod delay;
pub mod distortion;
pub mod dynamics;
pub mod eq;
pub mod filter;
pub mod gate;
pub mod modulation;
pub mod multiband;
pub mod reverb;
pub mod utility;

use soundesti_plugin_api::{Effect, ParamInfo};

pub struct EffectInfo {
    pub type_id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamInfo],
}

pub static CATALOG: &[EffectInfo] = &[
    EffectInfo {
        type_id: eq::TYPE_ID,
        name: "EQ",
        category: "EQ & Filter",
        description: "5-band equalizer: low cut, low shelf, mid peak, high shelf, high cut",
        params: &eq::PARAMS,
    },
    EffectInfo {
        type_id: filter::TYPE_ID,
        name: "Filter",
        category: "EQ & Filter",
        description: "Resonant low/high/band-pass filter for sweeps",
        params: &filter::PARAMS,
    },
    EffectInfo {
        type_id: dynamics::COMP_ID,
        name: "Compressor",
        category: "Dynamics",
        description: "Compressor with threshold, ratio, knee, attack, release, makeup",
        params: &dynamics::COMP_PARAMS,
    },
    EffectInfo {
        type_id: multiband::MULTIBAND_ID,
        name: "Multiband Compressor",
        category: "Dynamics",
        description: "Three-band compressor with adjustable crossovers and band gains",
        params: &multiband::MULTIBAND_PARAMS,
    },
    EffectInfo {
        type_id: gate::TYPE_ID,
        name: "Gate",
        category: "Dynamics",
        description: "Noise gate with hold and range",
        params: &gate::PARAMS,
    },
    EffectInfo {
        type_id: dynamics::LIMIT_ID,
        name: "Limiter",
        category: "Dynamics",
        description: "Peak limiter with input gain and output ceiling",
        params: &dynamics::LIMIT_PARAMS,
    },
    EffectInfo {
        type_id: delay::TYPE_ID,
        name: "Delay",
        category: "Delay & Reverb",
        description: "Stereo/ping-pong delay, tempo sync, feedback tone",
        params: &delay::PARAMS,
    },
    EffectInfo {
        type_id: reverb::TYPE_ID,
        name: "Reverb",
        category: "Delay & Reverb",
        description: "Algorithmic room/hall reverb",
        params: &reverb::PARAMS,
    },
    EffectInfo {
        type_id: modulation::PHASER_ID,
        name: "Phaser",
        category: "Modulation",
        description: "Swept allpass phaser, 4-8 stages",
        params: &modulation::PHASER_PARAMS,
    },
    EffectInfo {
        type_id: modulation::FLANGER_ID,
        name: "Flanger",
        category: "Modulation",
        description: "Short modulated delay with feedback",
        params: &modulation::FLANGER_PARAMS,
    },
    EffectInfo {
        type_id: chorus::TYPE_ID,
        name: "Chorus",
        category: "Modulation",
        description: "Stereo chorus",
        params: &chorus::PARAMS,
    },
    EffectInfo {
        type_id: distortion::TYPE_ID,
        name: "Distortion",
        category: "Distortion",
        description: "Soft clip, hard clip, wavefold and bitcrush with tone control",
        params: &distortion::PARAMS,
    },
    EffectInfo {
        type_id: multiband::POLISH_ID,
        name: "Polish",
        category: "Enhance",
        description: "One-knob enhancer: band compression, warmth and air",
        params: &multiband::POLISH_PARAMS,
    },
    EffectInfo {
        type_id: utility::TYPE_ID,
        name: "Utility",
        category: "Utility",
        description: "Gain, pan and stereo width",
        params: &utility::PARAMS,
    },
];

pub fn create(type_id: &str, sample_rate: f32) -> Option<Box<dyn Effect>> {
    Some(match type_id {
        eq::TYPE_ID => Box::new(eq::Eq::new(sample_rate)),
        filter::TYPE_ID => Box::new(filter::Filter::new(sample_rate)),
        dynamics::COMP_ID => Box::new(dynamics::Compressor::new(sample_rate)),
        dynamics::LIMIT_ID => Box::new(dynamics::Limiter::new(sample_rate)),
        delay::TYPE_ID => Box::new(delay::Delay::new(sample_rate)),
        reverb::TYPE_ID => Box::new(reverb::Reverb::new(sample_rate)),
        chorus::TYPE_ID => Box::new(chorus::Chorus::new(sample_rate)),
        distortion::TYPE_ID => Box::new(distortion::Distortion::new(sample_rate)),
        utility::TYPE_ID => Box::new(utility::Utility::new(sample_rate)),
        gate::TYPE_ID => Box::new(gate::Gate::new(sample_rate)),
        modulation::PHASER_ID => Box::new(modulation::Phaser::new(sample_rate)),
        modulation::FLANGER_ID => Box::new(modulation::Flanger::new(sample_rate)),
        multiband::MULTIBAND_ID => Box::new(multiband::Multiband::new(sample_rate)),
        multiband::POLISH_ID => Box::new(multiband::Polish::new(sample_rate)),
        _ => return None,
    })
}

pub fn info(type_id: &str) -> Option<&'static EffectInfo> {
    CATALOG.iter().find(|e| e.type_id == type_id)
}

pub fn params_for(type_id: &str) -> &'static [ParamInfo] {
    info(type_id).map(|e| e.params).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_dsp::Noise;

    const SR: f32 = 48_000.0;

    fn noise_block(n: usize, amp: f32) -> (Vec<f32>, Vec<f32>) {
        let mut ns = Noise::new(5);
        let l: Vec<f32> = (0..n).map(|_| ns.next() * amp).collect();
        let r: Vec<f32> = (0..n).map(|_| ns.next() * amp).collect();
        (l, r)
    }

    #[test]
    fn every_effect_is_stable_and_silent_in_silent_out() {
        for info in CATALOG {
            let mut fx = create(info.type_id, SR).unwrap();
            assert_eq!(fx.type_id(), info.type_id);
            let mut keys = std::collections::HashSet::new();
            for p in info.params {
                assert!(keys.insert(p.key), "{}: duplicate {}", info.name, p.key);
                assert!(p.min <= p.default && p.default <= p.max, "{}: bad default {}", info.name, p.key);
            }
            // Silence stays silence.
            let mut l = vec![0.0; 2048];
            let mut r = vec![0.0; 2048];
            fx.process(&mut l, &mut r);
            assert!(l.iter().chain(r.iter()).all(|x| x.abs() < 1e-6), "{} makes noise from silence", info.name);
            // Extreme settings never produce NaN/inf or blow up (the max EQ boost is ~60 dB, so
            // feed quiet noise and allow that much legitimate gain).
            for p in info.params {
                fx.set_param(p.id, p.max);
            }
            let (mut l, mut r) = noise_block(48_000, 0.05);
            fx.process(&mut l, &mut r);
            assert!(
                l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() < 100.0),
                "{} unstable at max settings",
                info.name
            );
            for p in info.params {
                fx.set_param(p.id, p.min);
                fx.set_param(p.id, f32::NAN);
            }
            let (mut l, mut r) = noise_block(4_800, 1.0);
            fx.process(&mut l, &mut r);
            assert!(l.iter().chain(r.iter()).all(|x| x.is_finite()), "{} unstable at min settings", info.name);
        }
    }
}
