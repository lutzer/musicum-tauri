use audio_plugin_sdk::PluginRegistry;
use structural_processor_sdk::Registry as StructuralRegistry;

/// Combined registry of all known structural processors and audio plugins.
/// Pass an instance to `PlaybackEngine::new`.
pub struct EditRegistry {
    pub structural: StructuralRegistry,
    pub plugins:    PluginRegistry,
}

impl Default for EditRegistry {
    /// Registers all built-in structural processors and audio plugins.
    fn default() -> Self {
        use audio_plugin_sdk::PluginEntry;

        let structural = structural_processors::registry();

        let mut plugins = PluginRegistry::new();
        plugins.insert("gain".into(),        PluginEntry::of::<plugin_gain::GainPlugin>());
        plugins.insert("reverb".into(),      PluginEntry::of::<plugin_reverb::ReverbPlugin>());
        plugins.insert("pan".into(),         PluginEntry::of::<plugin_pan::PanPlugin>());
        plugins.insert("normalize".into(),   PluginEntry::of::<plugin_normalize::NormalizePlugin>());
        plugins.insert("level-meter".into(), PluginEntry::of::<plugin_level_meter::LevelMeter>());
        plugins.insert("oscilloscope".into(),PluginEntry::of::<plugin_oscilloscope::OscilloscopePlugin>());

        Self { structural, plugins }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_all_structural_processors() {
        let reg = EditRegistry::default();
        for id in ["trim", "cut", "slice", "crop"] {
            assert!(reg.structural.contains_key(id), "missing structural '{id}'");
        }
    }

    #[test]
    fn default_registry_has_all_plugins() {
        let reg = EditRegistry::default();
        for id in ["gain", "reverb", "pan", "normalize", "level-meter", "oscilloscope"] {
            assert!(reg.plugins.contains_key(id), "missing plugin '{id}'");
        }
    }

    #[test]
    fn plugin_entry_create_works() {
        let reg = EditRegistry::default();
        let entry = &reg.plugins["gain"];
        let instance = (entry.create)();
        // GainPlugin default gain=1.0
        assert_eq!(instance.get_parameter("gain"), 1.0);
    }
}
