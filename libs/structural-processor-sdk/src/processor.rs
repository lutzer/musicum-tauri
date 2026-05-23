use std::collections::HashMap;

use serde::Serialize;

pub type Params = HashMap<String, f64>;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ParameterDescriptor {
    Time { id: &'static str, name: &'static str, default: f64 },
    Int { id: &'static str, name: &'static str, default: i64, min: i64, max: i64 },
}

#[derive(Serialize)]
pub struct ProcessorDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub parameters: &'static [ParameterDescriptor],
}

impl ProcessorDescriptor {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("descriptor serialisation failed")
    }
}

/// Core trait implemented by every structural processor.
///
/// Audio is always interleaved f32 samples.
/// `channels` is used to interpret the interleaving (e.g. stereo: frame * 2 + ch).
pub trait StructuralProcessor {
    fn descriptor() -> &'static ProcessorDescriptor;

    /// Return `true` when `params` are valid for this processor.
    fn validate(params: &Params) -> bool;

    /// Apply the edit and return new (possibly shorter) interleaved f32 samples.
    fn apply(samples: &[f32], sample_rate: u32, channels: u16, params: &Params) -> Vec<f32>;

    /// Duration of the output audio (seconds) given input `duration` and `params`.
    fn output_duration(duration: f64, params: &Params) -> f64;

    /// Map a time in the *processed* domain back to the *source* domain.
    /// `duration` is the audio length (seconds) *before* this edit.
    fn map_time_back(t: f64, duration: f64, params: &Params) -> f64;

    /// Map a time in the *source* domain forward to the *processed* domain.
    /// `duration` is the audio length (seconds) *before* this edit.
    fn map_time_forward(t: f64, duration: f64, params: &Params) -> f64;
}
