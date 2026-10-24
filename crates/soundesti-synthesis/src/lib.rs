//! Soundesti native instruments.

pub mod trident;

pub use trident::Trident;

use soundesti_plugin_api::Instrument;

/// Creates a native instrument by its persistent type id.
pub fn create_instrument(type_id: &str, sample_rate: f32) -> Option<Box<dyn Instrument>> {
    match type_id {
        trident::TYPE_ID => Some(Box::new(Trident::new(sample_rate))),
        _ => None,
    }
}

/// Every native instrument type id, for the plugin picker.
pub const INSTRUMENT_TYPES: &[(&str, &str)] = &[(trident::TYPE_ID, "Trident")];
