use std::collections::HashMap;
use crate::parameters::PluginDescriptor;
use crate::plugin::PluginProcessor;

/// Vtable entry for one audio plugin. Static fn pointers allow instantiation
/// and descriptor queries without a concrete type in scope.
pub struct PluginEntry {
    pub descriptor: fn() -> &'static PluginDescriptor,
    pub create:     fn() -> Box<dyn PluginProcessor>,
}

impl PluginEntry {
    /// Build a `PluginEntry` from any `T: AudioPlugin + Send + 'static`.
    pub fn of<T: crate::plugin::AudioPlugin + Send + 'static>() -> Self {
        Self {
            descriptor: T::descriptor,
            create:     || Box::new(T::new()),
        }
    }
}

/// Registry of all known audio plugins. Built once at startup.
pub type PluginRegistry = HashMap<String, PluginEntry>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioPlugin, PluginDescriptor, PluginMode, PluginParameter};

    struct TinyPlugin;
    static TINY_PARAMS: [PluginParameter; 0] = [];
    static TINY_DESC: PluginDescriptor = PluginDescriptor {
        id: "tiny", name: "Tiny", version: "0",
        mode: PluginMode::Realtime, parameters: &TINY_PARAMS,
    };
    impl AudioPlugin for TinyPlugin {
        fn descriptor() -> &'static PluginDescriptor { &TINY_DESC }
        fn new() -> Self { TinyPlugin }
        fn set_parameter(&mut self, _: &str, _: f32) {}
        fn get_parameter(&self, _: &str) -> f32 { 0.0 }
    }

    #[test]
    fn plugin_entry_of_creates_instance() {
        let entry = PluginEntry::of::<TinyPlugin>();
        let mut instance = (entry.create)();
        instance.set_parameter("x", 1.0); // must not panic
        assert_eq!((entry.descriptor)().id, "tiny");
    }

    #[test]
    fn plugin_registry_lookup() {
        let mut reg: PluginRegistry = HashMap::new();
        reg.insert("tiny".to_string(), PluginEntry::of::<TinyPlugin>());
        assert!(reg.contains_key("tiny"));
        let e = &reg["tiny"];
        assert_eq!((e.descriptor)().id, "tiny");
    }
}
