//! Comics: shared sources, cache and library live in the core; the desktop
//! keeps AI translation and its settings surface.
pub mod translation;

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;

pub use pordo_core::comics::*;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub komiic: source::KomiicAccount,
    pub image_selected: crate::ai::ModelRef,
    #[serde(skip_serializing)]
    pub image_base_url: String,
    #[serde(skip_serializing)]
    pub image_model: String,
    #[serde(skip)]
    pub image_key: String,
    pub image_quality: String,
    pub target: String,
    pub mode: ReadingMode,
    pub check_updates: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            komiic: Default::default(),
            image_selected: Default::default(),
            image_base_url: String::new(),
            image_model: String::new(),
            image_key: String::new(),
            image_quality: String::new(),
            target: "auto".into(),
            mode: ReadingMode::Vertical,
            check_updates: true,
        }
    }
}
