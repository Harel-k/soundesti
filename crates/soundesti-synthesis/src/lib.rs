//! Soundesti native instruments.

pub mod chopper;
mod common;
pub mod drums;
pub mod harpa;
pub mod orbit;
pub mod padz;
pub mod sampler;
pub mod tide;
pub mod trident;

pub use drums::{Clapper, Hatz, Kickr};
pub use harpa::Harpa;
pub use orbit::Orbit;
pub use sampler::Sampler;
pub use trident::Trident;

use soundesti_plugin_api::{Instrument, ParamInfo};

/// One entry of the instrument catalog.
pub struct InstrumentInfo {
    pub type_id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub params: &'static [ParamInfo],
}

/// Every native instrument, in plugin-picker order.
pub static CATALOG: &[InstrumentInfo] = &[
    InstrumentInfo {
        type_id: sampler::TYPE_ID,
        name: "Sampler",
        category: "Samplers",
        description: "Plays an audio file: root key, pitch tracking, one-shot or gated, reverse",
        params: &sampler::PARAMS,
    },
    InstrumentInfo {
        type_id: chopper::TYPE_ID,
        name: "Chopper",
        category: "Samplers",
        description: "Loop slicer: cuts a sample into slices played from consecutive keys",
        params: &chopper::PARAMS,
    },
    InstrumentInfo {
        type_id: padz::TYPE_ID,
        name: "Padz",
        category: "Drums",
        description: "16-pad drum sampler: a sample, volume, pan and tune per pad",
        params: &padz::PARAMS,
    },
    InstrumentInfo {
        type_id: drums::kick::TYPE_ID,
        name: "Kickr",
        category: "Drums",
        description: "Kick drum synth: pitch sweep, click, drive",
        params: &drums::KICK_PARAMS,
    },
    InstrumentInfo {
        type_id: drums::clap::TYPE_ID,
        name: "Clapper",
        category: "Drums",
        description: "Snare and clap synth: tonal body plus filtered noise bursts",
        params: &drums::CLAP_PARAMS,
    },
    InstrumentInfo {
        type_id: drums::hat::TYPE_ID,
        name: "Hatz",
        category: "Drums",
        description: "Hi-hat synth: metallic square bank, choke for closed/open hats",
        params: &drums::HAT_PARAMS,
    },
    InstrumentInfo {
        type_id: trident::TYPE_ID,
        name: "Trident",
        category: "Synths",
        description: "3-oscillator subtractive synth with filter and envelope",
        params: &trident::PARAMS,
    },
    InstrumentInfo {
        type_id: orbit::TYPE_ID,
        name: "Orbit",
        category: "Synths",
        description: "2-operator FM synth: bells, keys, basses, plucks",
        params: &orbit::PARAMS,
    },
    InstrumentInfo {
        type_id: harpa::TYPE_ID,
        name: "Harpa",
        category: "Synths",
        description: "Plucked-string physical model: guitars, harps, plucks",
        params: &harpa::PARAMS,
    },
    InstrumentInfo {
        type_id: tide::TYPE_ID,
        name: "Tide",
        category: "Synths",
        description: "Wavetable synth: morphing waves, LFO, unison, filter",
        params: &tide::PARAMS,
    },
];

/// Creates a native instrument by its persistent type id.
pub fn create_instrument(type_id: &str, sample_rate: f32) -> Option<Box<dyn Instrument>> {
    Some(match type_id {
        trident::TYPE_ID => Box::new(Trident::new(sample_rate)),
        drums::kick::TYPE_ID => Box::new(Kickr::new(sample_rate)),
        drums::clap::TYPE_ID => Box::new(Clapper::new(sample_rate)),
        drums::hat::TYPE_ID => Box::new(Hatz::new(sample_rate)),
        orbit::TYPE_ID => Box::new(Orbit::new(sample_rate)),
        harpa::TYPE_ID => Box::new(Harpa::new(sample_rate)),
        sampler::TYPE_ID => Box::new(Sampler::new(sample_rate)),
        chopper::TYPE_ID => Box::new(chopper::Chopper::new(sample_rate)),
        tide::TYPE_ID => Box::new(tide::Tide::new(sample_rate)),
        padz::TYPE_ID => Box::new(padz::Padz::new(sample_rate)),
        _ => return None,
    })
}

pub fn info(type_id: &str) -> Option<&'static InstrumentInfo> {
    CATALOG.iter().find(|i| i.type_id == type_id)
}

/// Parameter table for an instrument type (empty if unknown).
pub fn params_for(type_id: &str) -> &'static [ParamInfo] {
    info(type_id).map(|i| i.params).unwrap_or(&[])
}

/// (type id, display name) pairs, kept for existing callers.
pub const INSTRUMENT_TYPES: &[(&str, &str)] = &[
    (sampler::TYPE_ID, "Sampler"),
    (drums::kick::TYPE_ID, "Kickr"),
    (drums::clap::TYPE_ID, "Clapper"),
    (drums::hat::TYPE_ID, "Hatz"),
    (trident::TYPE_ID, "Trident"),
    (orbit::TYPE_ID, "Orbit"),
    (harpa::TYPE_ID, "Harpa"),
    (chopper::TYPE_ID, "Chopper"),
    (tide::TYPE_ID, "Tide"),
    (padz::TYPE_ID, "Padz"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_plugin_api::{NoteEvent, TimedEvent};

    #[test]
    fn catalog_is_consistent_and_every_instrument_works() {
        assert_eq!(CATALOG.len(), INSTRUMENT_TYPES.len());
        for info in CATALOG {
            let mut inst =
                create_instrument(info.type_id, 48_000.0).unwrap_or_else(|| panic!("{} not creatable", info.type_id));
            assert_eq!(inst.type_id(), info.type_id);
            assert_eq!(inst.params().len(), info.params.len());
            let mut ids = std::collections::HashSet::new();
            let mut keys = std::collections::HashSet::new();
            for p in info.params {
                assert!(ids.insert(p.id) && keys.insert(p.key), "{}: duplicate param {}", info.name, p.key);
                assert!(p.min <= p.default && p.default <= p.max, "{}: bad default for {}", info.name, p.key);
                // Extreme values must never produce NaN.
                inst.set_param(p.id, f32::NAN);
                inst.set_param(p.id, p.max * 10.0);
                inst.set_param(p.id, p.default);
            }
            let mut l = vec![0.0; 4_800];
            let mut r = vec![0.0; 4_800];
            inst.process(
                &[TimedEvent { offset: 10, event: NoteEvent::On { id: 1, key: 60, velocity: 0.9 } }],
                &mut l,
                &mut r,
            );
            assert!(
                l.iter().chain(r.iter()).all(|x| x.is_finite() && x.abs() < 4.0),
                "{} output out of range",
                info.name
            );
            // Sample-based instruments are silent until they get a sample.
            if ![sampler::TYPE_ID, chopper::TYPE_ID, padz::TYPE_ID].contains(&info.type_id) {
                assert!(l.iter().any(|x| x.abs() > 1e-3), "{} made no sound", info.name);
            }
        }
    }

    #[test]
    fn melodic_instruments_release_on_note_off() {
        for ty in [orbit::TYPE_ID, harpa::TYPE_ID, trident::TYPE_ID] {
            let mut inst = create_instrument(ty, 48_000.0).unwrap();
            let mut l = vec![0.0; 2_400];
            let mut r = vec![0.0; 2_400];
            inst.process(
                &[TimedEvent { offset: 0, event: NoteEvent::On { id: 7, key: 64, velocity: 1.0 } }],
                &mut l,
                &mut r,
            );
            let mut l = vec![0.0; 480_000];
            let mut r = vec![0.0; 480_000];
            inst.process(&[TimedEvent { offset: 0, event: NoteEvent::Off { id: 7 } }], &mut l, &mut r);
            assert_eq!(inst.active_voices(), 0, "{ty} kept sounding after release");
        }
    }
}
