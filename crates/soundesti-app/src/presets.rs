//! Parameter presets for native instruments and effects: a few built-in starting points
//! per type plus user presets saved as JSON in %APPDATA%\Soundesti\Presets\<type>\.

use std::collections::BTreeMap;
use std::path::PathBuf;

use egui::{RichText, Ui};

use crate::theme;

pub struct Preset {
    pub name: String,
    pub factory: bool,
    pub params: BTreeMap<String, f32>,
}

/// Built-in presets: (type id, name, [(param key, value)]).
type Factory = (&'static str, &'static str, &'static [(&'static str, f32)]);

const FACTORY: &[Factory] = &[
    (
        "soundesti.tide",
        "Glass Pad",
        &[
            ("wave.position", 0.15),
            ("wave.lfo_amount", 0.35),
            ("wave.lfo_rate", 0.2),
            ("unison.voices", 3.0),
            ("unison.detune", 9.0),
            ("amp.attack", 0.6),
            ("amp.release", 1.2),
            ("filter.cutoff", 6000.0),
        ],
    ),
    (
        "soundesti.tide",
        "Super Lead",
        &[
            ("wave.position", 0.57),
            ("unison.voices", 5.0),
            ("unison.detune", 22.0),
            ("amp.attack", 0.005),
            ("amp.sustain", 0.8),
            ("filter.cutoff", 9000.0),
        ],
    ),
    (
        "soundesti.tide",
        "Pulse Bass",
        &[
            ("wave.position", 0.86),
            ("unison.voices", 1.0),
            ("filter.cutoff", 900.0),
            ("filter.resonance", 2.5),
            ("amp.decay", 0.25),
            ("amp.sustain", 0.4),
        ],
    ),
    (
        "soundesti.orbit",
        "Bell",
        &[("mod.ratio", 3.5), ("mod.index", 4.0), ("mod.decay", 1.5), ("amp.decay", 2.0), ("amp.sustain", 0.0)],
    ),
    (
        "soundesti.orbit",
        "Electric Piano",
        &[("mod.ratio", 1.0), ("mod.index", 1.8), ("mod.decay", 0.8), ("amp.decay", 1.5), ("amp.sustain", 0.2)],
    ),
    (
        "soundesti.orbit",
        "FM Bass",
        &[("mod.ratio", 1.0), ("mod.index", 6.0), ("mod.decay", 0.4), ("mod.feedback", 0.4), ("amp.sustain", 0.7)],
    ),
    (
        "soundesti.trident",
        "Warm Bass",
        &[("filter.cutoff", 700.0), ("filter.resonance", 1.5), ("amp.decay", 0.4), ("amp.sustain", 0.5)],
    ),
    (
        "soundesti.trident",
        "Bright Lead",
        &[("filter.cutoff", 6000.0), ("filter.resonance", 1.2), ("amp.sustain", 0.85)],
    ),
    ("soundesti.harpa", "Nylon", &[("string.brightness", 0.35), ("string.sustain", 2.0), ("string.pick", 0.5)]),
    ("soundesti.harpa", "Bright Pluck", &[("string.brightness", 0.85), ("string.sustain", 1.2), ("string.pick", 0.9)]),
    (
        "soundesti.kickr",
        "808",
        &[("pitch.start", 120.0), ("pitch.end", 44.0), ("pitch.decay", 0.12), ("amp.decay", 1.8), ("amp.click", 0.15)],
    ),
    (
        "soundesti.kickr",
        "Punchy",
        &[
            ("pitch.start", 260.0),
            ("pitch.end", 50.0),
            ("pitch.decay", 0.04),
            ("amp.decay", 0.35),
            ("amp.click", 0.6),
            ("amp.drive", 0.4),
        ],
    ),
    ("soundesti.reverb", "Small Room", &[("reverb.size", 0.35), ("reverb.damping", 0.6), ("reverb.mix", 0.18)]),
    (
        "soundesti.reverb",
        "Big Hall",
        &[("reverb.size", 0.92), ("reverb.damping", 0.3), ("reverb.predelay", 40.0), ("reverb.mix", 0.3)],
    ),
    ("soundesti.delay", "Dotted Eighth", &[("delay.sync", 3.0), ("delay.feedback", 0.45), ("delay.mix", 0.3)]),
    (
        "soundesti.delay",
        "Slapback",
        &[
            ("delay.sync", 0.0),
            ("delay.time", 110.0),
            ("delay.feedback", 0.1),
            ("delay.pingpong", 0.0),
            ("delay.mix", 0.35),
        ],
    ),
    (
        "soundesti.compressor",
        "Gentle Glue",
        &[
            ("comp.threshold", -12.0),
            ("comp.ratio", 2.0),
            ("comp.attack", 30.0),
            ("comp.release", 200.0),
            ("comp.makeup", 2.0),
        ],
    ),
    (
        "soundesti.compressor",
        "Drum Smash",
        &[
            ("comp.threshold", -30.0),
            ("comp.ratio", 10.0),
            ("comp.attack", 1.0),
            ("comp.release", 80.0),
            ("comp.makeup", 10.0),
            ("comp.mix", 0.5),
        ],
    ),
    (
        "soundesti.eq",
        "Vocal Clarity",
        &[("lowcut.freq", 90.0), ("mid.freq", 3000.0), ("mid.gain", 3.0), ("high.gain", 2.0)],
    ),
    ("soundesti.eq", "Mud Cut", &[("mid.freq", 300.0), ("mid.gain", -4.0), ("mid.q", 1.2)]),
];

fn user_dir(type_id: &str) -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|d| PathBuf::from(d).join("Soundesti").join("Presets").join(type_id.replace(['.', '/', '\\'], "_")))
}

fn safe(name: &str) -> String {
    name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect::<String>().trim().to_string()
}

pub fn list(type_id: &str) -> Vec<Preset> {
    let mut out: Vec<Preset> = FACTORY
        .iter()
        .filter(|(t, _, _)| *t == type_id)
        .map(|(_, name, params)| Preset {
            name: name.to_string(),
            factory: true,
            params: params.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        })
        .collect();
    if let Some(dir) = user_dir(type_id)
        && let Ok(rd) = std::fs::read_dir(dir)
    {
        let mut user: Vec<Preset> = rd
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let params: BTreeMap<String, f32> =
                    serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
                Some(Preset { name: e.path().file_stem()?.to_string_lossy().into_owned(), factory: false, params })
            })
            .collect();
        user.sort_by_key(|p| p.name.to_lowercase());
        out.extend(user);
    }
    out
}

pub fn save(type_id: &str, name: &str, params: &BTreeMap<String, f32>) -> Result<PathBuf, String> {
    let name = safe(name);
    if name.is_empty() {
        return Err("give the preset a name".into());
    }
    let dir = user_dir(type_id).ok_or("no settings folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{name}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(params).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn delete(type_id: &str, name: &str) {
    if let Some(dir) = user_dir(type_id) {
        let _ = std::fs::remove_file(dir.join(format!("{}.json", safe(name))));
    }
}

pub enum Action {
    /// Replace all parameters with these (missing keys go back to defaults).
    Load(BTreeMap<String, f32>),
    Saved(String),
    Error(String),
}

/// The presets bar: a menu of presets, "Save as", and the name field while saving.
/// `naming` holds the in-progress name for this window (None when not saving).
pub fn preset_bar(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    type_id: &str,
    params: &BTreeMap<String, f32>,
    naming: &mut Option<String>,
) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        let presets = list(type_id);
        ui.menu_button(RichText::new("Presets").small(), |ui| {
            if presets.is_empty() {
                ui.label(RichText::new("No presets yet").small().color(theme::TEXT_DIM));
            }
            let mut last_factory = None;
            for p in &presets {
                if last_factory.is_some_and(|f| f != p.factory) {
                    ui.separator();
                }
                last_factory = Some(p.factory);
                ui.horizontal(|ui| {
                    if ui.button(&p.name).clicked() {
                        action = Some(Action::Load(p.params.clone()));
                        ui.close();
                    }
                    if !p.factory && ui.small_button("x").on_hover_text("Delete this preset").clicked() {
                        delete(type_id, &p.name);
                        ui.close();
                    }
                });
            }
        });
        match naming {
            None => {
                if ui.small_button("Save preset…").clicked() {
                    *naming = Some(String::new());
                }
            }
            Some(name) => {
                let r = ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text("Preset name")
                        .desired_width(140.0)
                        .id_salt(("preset_name", &salt)),
                );
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.small_button("Save").clicked() || enter {
                    action = Some(match save(type_id, name, params) {
                        Ok(_) => Action::Saved(name.clone()),
                        Err(e) => Action::Error(e),
                    });
                    *naming = None;
                } else if ui.small_button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    *naming = None;
                }
            }
        }
    });
    action
}

impl crate::app::App {
    /// Draws the presets bar for one window; returns parameters to load, if chosen.
    pub fn preset_bar_for(
        &mut self,
        ui: &mut Ui,
        key: String,
        type_id: &str,
        params: &BTreeMap<String, f32>,
    ) -> Option<BTreeMap<String, f32>> {
        let mut naming = self.preset_naming.remove(&key);
        let action = preset_bar(ui, &key, type_id, params, &mut naming);
        if let Some(n) = naming {
            self.preset_naming.insert(key, n);
        }
        match action? {
            Action::Load(p) => Some(p),
            Action::Saved(name) => {
                self.set_status(format!("Saved preset \"{name}\"."));
                None
            }
            Action::Error(e) => {
                self.set_status(format!("Could not save the preset: {e}"));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_presets_use_real_parameter_keys() {
        for (type_id, name, params) in FACTORY {
            let known: Vec<&str> = soundesti_synthesis::params_for(type_id)
                .iter()
                .chain(soundesti_effects::params_for(type_id))
                .map(|p| p.key)
                .collect();
            assert!(!known.is_empty(), "{type_id} is not a native instrument or effect");
            for (k, _) in params.iter() {
                assert!(known.contains(k), "preset {name} ({type_id}) uses unknown key {k}");
            }
        }
    }
}
