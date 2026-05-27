mod analyzer;
mod parameters;
mod plugin;
pub mod registry;

pub use analyzer::AudioAnalyzer;
pub use hound;
pub use parameters::{
    BoolParam, FloatParam, ParamMap, ParamResult,
    PluginDescriptor, PluginMode, PluginParameter,
};
pub use plugin::{AudioPlugin, PluginProcessor};
pub use registry::{PluginEntry, PluginRegistry};

#[cfg(test)]
mod tests;
