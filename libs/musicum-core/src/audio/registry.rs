use audio_plugin_sdk::{PluginEntry, PluginParameter, PluginRegistry};
use structural_processor_sdk::{processor::ParameterDescriptor, Registry as StructuralRegistry};

/// Parameter metadata exposed to frontends without importing plugin/processor SDKs.
/// `Action` and `Canvas` plugin parameters are excluded (no persistent value).
pub enum ParamInfo {
    Float {
        id:      &'static str,
        name:    &'static str,
        default: f32,
        min:     f32,
        max:     f32,
        step:    f32,
        unit:    Option<&'static str>,
    },
    Bool  { id: &'static str, name: &'static str, default: bool },
    Time  { id: &'static str, name: &'static str, default: f64 },
    Int   { id: &'static str, name: &'static str, default: i64, min: i64, max: i64 },
}

pub enum EditType { Structural, Plugin }

pub struct EditEntry {
    pub id:         String,
    pub name:       &'static str,
    pub edit_type:  EditType,
    pub parameters: Vec<ParamInfo>,
}

/// Combined registry of all known structural processors and audio plugins.
/// Pass an instance to `PlaybackEngine::new`.
pub struct EditRegistry {
    pub structural: StructuralRegistry,
    pub plugins:    PluginRegistry,
}

impl Default for EditRegistry {
    /// Registers all built-in structural processors and audio plugins.
    fn default() -> Self {
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

impl EditRegistry {
    /// Return all registered processors and plugins as frontend-safe entries.
    pub fn list_entries(&self) -> Vec<EditEntry> {
        let mut entries = Vec::new();

        for (id, entry) in &self.structural {
            let d = (entry.descriptor)();
            let parameters = d.parameters.iter().map(|p| match p {
                ParameterDescriptor::Time { id, name, default } =>
                    ParamInfo::Time { id, name, default: *default },
                ParameterDescriptor::Int { id, name, default, min, max } =>
                    ParamInfo::Int { id, name, default: *default, min: *min, max: *max },
            }).collect();
            entries.push(EditEntry {
                id: id.clone(),
                name: d.name,
                edit_type: EditType::Structural,
                parameters,
            });
        }

        for (id, entry) in &self.plugins {
            let d = (entry.descriptor)();
            let parameters = d.parameters.iter().filter_map(|p| match p {
                PluginParameter::Float { id, name, default, min, max, step, unit, .. } =>
                    Some(ParamInfo::Float {
                        id, name,
                        default: *default, min: *min, max: *max, step: *step,
                        unit: if unit.is_empty() { None } else { Some(unit) },
                    }),
                PluginParameter::Bool { id, name, default, .. } =>
                    Some(ParamInfo::Bool { id, name, default: *default }),
                PluginParameter::Action { .. } | PluginParameter::Canvas { .. } => None,
            }).collect();
            entries.push(EditEntry {
                id: id.clone(),
                name: d.name,
                edit_type: EditType::Plugin,
                parameters,
            });
        }

        entries
    }

    /// Look up a single entry by processor or plugin ID.
    pub fn get_entry(&self, id: &str) -> Option<EditEntry> {
        self.list_entries().into_iter().find(|e| e.id == id)
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

    #[test]
    fn list_entries_contains_all_structural() {
        let reg = EditRegistry::default();
        let entries = reg.list_entries();
        for id in ["trim", "cut", "slice", "crop"] {
            assert!(
                entries.iter().any(|e| e.id == id && matches!(e.edit_type, EditType::Structural)),
                "missing structural entry '{id}'"
            );
        }
    }

    #[test]
    fn list_entries_contains_all_plugins() {
        let reg = EditRegistry::default();
        let entries = reg.list_entries();
        for id in ["gain", "reverb", "pan", "normalize", "level-meter", "oscilloscope"] {
            assert!(
                entries.iter().any(|e| e.id == id && matches!(e.edit_type, EditType::Plugin)),
                "missing plugin entry '{id}'"
            );
        }
    }

    #[test]
    fn get_entry_gain_has_float_param() {
        let reg = EditRegistry::default();
        let entry = reg.get_entry("gain").unwrap();
        assert!(entry.parameters.iter().any(|p| matches!(p, ParamInfo::Float { id, .. } if *id == "gain")));
    }

    #[test]
    fn get_entry_trim_has_time_params() {
        let reg = EditRegistry::default();
        let entry = reg.get_entry("trim").unwrap();
        assert!(entry.parameters.iter().any(|p| matches!(p, ParamInfo::Time { id, .. } if *id == "start")));
        assert!(entry.parameters.iter().any(|p| matches!(p, ParamInfo::Time { id, .. } if *id == "end")));
    }

    #[test]
    fn get_entry_unknown_returns_none() {
        let reg = EditRegistry::default();
        assert!(reg.get_entry("nonexistent").is_none());
    }
}
