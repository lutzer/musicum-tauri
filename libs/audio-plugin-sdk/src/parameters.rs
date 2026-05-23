use std::collections::HashMap;
use serde::Serialize;

pub(crate) fn is_false(b: &bool) -> bool {
    !b
}

/// Execution mode of a plugin, encoded in the descriptor.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginMode {
    Realtime,
    Offline,
    Analyzed,
}

/// A typed parameter descriptor variant.
///
/// - `Float`  — a knob/slider with min/max/step.
/// - `Bool`   — an on/off toggle.
/// - `Action` — a trigger button with no persistent value (e.g. "Compute").
/// - `Canvas` — a canvas element for plugin-driven rendering.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginParameter {
    Float {
        id: &'static str,
        name: &'static str,
        min: f32,
        max: f32,
        default: f32,
        step: f32,
        /// Display unit shown next to the value, e.g. `"x"`, `"dBFS"`, `"s"`.
        unit: &'static str,
        #[serde(skip_serializing_if = "is_false")]
        disabled: bool,
        /// If `true`, hide this parameter from the plugin rack UI.
        #[serde(skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Bool {
        id: &'static str,
        name: &'static str,
        default: bool,
        #[serde(skip_serializing_if = "is_false")]
        disabled: bool,
        /// If `true`, hide this parameter from the plugin rack UI.
        #[serde(skip_serializing_if = "is_false")]
        hidden: bool,
    },
    Action {
        id: &'static str,
        name: &'static str,
        #[serde(skip_serializing_if = "is_false")]
        disabled: bool,
    },
    Canvas {
        id: &'static str,
        name: &'static str,
        /// Defines the height of the canvas element relative to its width; 1 = square.
        aspect_ratio: f32,
        #[serde(skip_serializing_if = "is_false")]
        disabled: bool,
    },
}

/// A runtime value holder for a `Float` plugin parameter.
///
/// Initialized from the descriptor's `default`, `min`, and `max`.
/// [`set`](Self::set) automatically clamps the incoming value to `[min, max]`.
pub struct FloatParam {
    value: f32,
    min: f32,
    max: f32,
}

impl FloatParam {
    /// Create a new holder initialised to `default`, clamped to `[min, max]`.
    pub fn new(default: f32, min: f32, max: f32) -> Self {
        FloatParam {
            value: default.clamp(min, max),
            min,
            max,
        }
    }

    /// Return the current value.
    pub fn get(&self) -> f32 {
        self.value
    }

    /// Set a new value, clamping to `[min, max]`.
    pub fn set(&mut self, v: f32) {
        self.value = v.clamp(self.min, self.max);
    }
}

/// A runtime value holder for a `Bool` plugin parameter.
///
/// Stores the value as a `bool`; exposes `f32` via `get()`/`set()` so it fits
/// the `AudioPlugin::set_parameter` / `get_parameter` interface.
pub struct BoolParam {
    value: bool,
}

impl BoolParam {
    /// Create a new holder initialised to `default`.
    pub fn new(default: bool) -> Self {
        BoolParam { value: default }
    }

    /// Return `1.0` if true, `0.0` if false.
    pub fn get(&self) -> f32 {
        if self.value {
            1.0
        } else {
            0.0
        }
    }

    /// Return the raw `bool` value.
    pub fn get_bool(&self) -> bool {
        self.value
    }

    /// Set from an `f32`: any non-zero value is `true`.
    pub fn set(&mut self, v: f32) {
        self.value = v != 0.0;
    }
}

impl PluginParameter {
    /// Create a [`FloatParam`] initialised from this variant's `default`, `min`, and `max`.
    ///
    /// # Panics
    /// Panics if called on a non-`Float` variant.
    pub fn float_param(&self) -> FloatParam {
        match self {
            PluginParameter::Float {
                default, min, max, ..
            } => FloatParam::new(*default, *min, *max),
            _ => panic!("float_param() called on a non-Float PluginParameter"),
        }
    }

    /// Create a [`BoolParam`] initialised from this variant's `default`.
    ///
    /// # Panics
    /// Panics if called on a non-`Bool` variant.
    pub fn bool_param(&self) -> BoolParam {
        match self {
            PluginParameter::Bool { default, .. } => BoolParam::new(*default),
            _ => panic!("bool_param() called on a non-Bool PluginParameter"),
        }
    }
}

/// Top-level plugin descriptor with all static metadata.
///
/// Define this as a `static` in each plugin and return a reference from
/// [`AudioPlugin::descriptor`](crate::AudioPlugin::descriptor).
#[derive(Serialize)]
pub struct PluginDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub mode: PluginMode,
    pub parameters: &'static [PluginParameter],
}

impl PluginDescriptor {
    /// Serialize this descriptor to a JSON string.
    ///
    /// Called once by the macro-generated C ABI on first access; the result is
    /// cached for the lifetime of the WASM module.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("PluginDescriptor serialization is infallible")
    }

    /// Parse `json` into a [`ParamMap`] backed by this descriptor's parameters.
    ///
    /// - `Float` entries: extracted as `f64`, stored as `f32`.
    /// - `Bool` entries: stored as `1.0` / `0.0`.
    /// - `Action` / `Canvas` entries: ignored (no persistent value).
    /// - Malformed JSON: treated as empty; [`ParamMap::get_float`] /
    ///   [`ParamMap::get_bool`] fall back to descriptor defaults.
    pub fn parse_params(&self, json: &str) -> ParamMap {
        let raw: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(json).unwrap_or_default();
        let mut values = HashMap::new();
        for param in self.parameters {
            match param {
                PluginParameter::Float { id, .. } => {
                    if let Some(v) = raw.get(*id).and_then(|v| v.as_f64()) {
                        values.insert(id.to_string(), v as f32);
                    }
                }
                PluginParameter::Bool { id, .. } => {
                    if let Some(v) = raw.get(*id).and_then(|v| v.as_bool()) {
                        values.insert(id.to_string(), if v { 1.0 } else { 0.0 });
                    }
                }
                _ => {}
            }
        }
        ParamMap { params: self.parameters, values }
    }
}

/// Typed parameter values parsed from a JSON payload, backed by a descriptor
/// for default-value fallback.
///
/// Constructed by [`PluginDescriptor::parse_params`]; not publicly constructable.
pub struct ParamMap {
    params: &'static [PluginParameter],
    values: HashMap<String, f32>,
}

impl ParamMap {
    /// Return the float value for `id`.
    ///
    /// Priority: parsed JSON value → descriptor `Float.default` → `0.0`.
    pub fn get_float(&self, id: &str) -> f32 {
        if let Some(&v) = self.values.get(id) {
            return v;
        }
        for param in self.params {
            if let PluginParameter::Float { id: pid, default, .. } = param {
                if *pid == id {
                    return *default;
                }
            }
        }
        0.0
    }

    /// Return the bool value for `id`.
    ///
    /// Priority: parsed JSON value → descriptor `Bool.default` → `false`.
    pub fn get_bool(&self, id: &str) -> bool {
        if let Some(&v) = self.values.get(id) {
            return v != 0.0;
        }
        for param in self.params {
            if let PluginParameter::Bool { id: pid, default, .. } = param {
                if *pid == id {
                    return *default;
                }
            }
        }
        false
    }
}

/// Typed output of [`AudioAnalyzer::finish_analysis`].
///
/// Build with [`ParamResult::with`]; the `implement_analyzer!` macro calls
/// [`to_json`](Self::to_json) to produce the ABI result string.
pub struct ParamResult {
    values: Vec<(String, f32)>,
}

impl ParamResult {
    pub fn new() -> Self {
        ParamResult { values: Vec::new() }
    }

    /// Append a named value and return `self` for chaining.
    pub fn with(mut self, id: &str, value: f32) -> Self {
        self.values.push((id.to_string(), value));
        self
    }

    /// Serialize to `{"key":value,...}` JSON.
    ///
    /// `pub` because `implement_analyzer!` expands in downstream crates.
    pub fn to_json(&self) -> String {
        let map: serde_json::Map<String, serde_json::Value> = self
            .values
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::from(*v as f64)))
            .collect();
        serde_json::to_string(&map).expect("ParamResult serialization is infallible")
    }
}

#[cfg(test)]
mod param_map_tests {
    use super::*;

    static TEST_PARAMS: [PluginParameter; 3] = [
        PluginParameter::Float {
            id: "gain",
            name: "Gain",
            min: 0.0,
            max: 2.0,
            default: 1.0,
            step: 0.01,
            unit: "x",
            disabled: false,
            hidden: false,
        },
        PluginParameter::Bool {
            id: "active",
            name: "Active",
            default: true,
            disabled: false,
            hidden: false,
        },
        PluginParameter::Action { id: "reset", name: "Reset", disabled: false },
    ];

    static TEST_DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: "test",
        name: "Test",
        version: "0.1.0",
        mode: PluginMode::Realtime,
        parameters: &TEST_PARAMS,
    };

    // --- ParamMap ---

    #[test]
    fn param_map_get_float_present() {
        let p = TEST_DESCRIPTOR.parse_params(r#"{"gain":1.5}"#);
        assert!((p.get_float("gain") - 1.5).abs() < 1e-6);
    }

    #[test]
    fn param_map_get_float_missing_uses_default() {
        let p = TEST_DESCRIPTOR.parse_params("{}");
        assert!((p.get_float("gain") - 1.0).abs() < 1e-6); // default = 1.0
    }

    #[test]
    fn param_map_get_float_unknown_id_returns_zero() {
        let p = TEST_DESCRIPTOR.parse_params("{}");
        assert!((p.get_float("unknown") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn param_map_get_bool_present() {
        let p = TEST_DESCRIPTOR.parse_params(r#"{"active":false}"#);
        assert!(!p.get_bool("active"));
    }

    #[test]
    fn param_map_get_bool_missing_uses_default() {
        let p = TEST_DESCRIPTOR.parse_params("{}");
        assert!(p.get_bool("active")); // default = true
    }

    #[test]
    fn param_map_get_bool_unknown_id_returns_false() {
        let p = TEST_DESCRIPTOR.parse_params("{}");
        assert!(!p.get_bool("unknown"));
    }

    #[test]
    fn param_map_malformed_json_uses_defaults() {
        let p = TEST_DESCRIPTOR.parse_params("not json");
        assert!((p.get_float("gain") - 1.0).abs() < 1e-6);
        assert!(p.get_bool("active"));
    }

    #[test]
    fn param_map_action_param_ignored() {
        // parsing never stores Action params; get_float on action id returns 0.0
        let p = TEST_DESCRIPTOR.parse_params(r#"{"reset":1}"#);
        assert!((p.get_float("reset") - 0.0).abs() < 1e-6);
    }

    // --- ParamResult ---

    #[test]
    fn param_result_empty_to_json() {
        let r = ParamResult::new();
        assert_eq!(r.to_json(), "{}");
    }

    #[test]
    fn param_result_single_value() {
        let r = ParamResult::new().with("computed_gain", 1.4159);
        let json = r.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!((v["computed_gain"].as_f64().unwrap() - 1.4159_f64).abs() < 1e-4);
    }

    #[test]
    fn param_result_multiple_values() {
        let r = ParamResult::new().with("a", 1.0).with("b", 2.0);
        let json = r.to_json();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!((v["a"].as_f64().unwrap() - 1.0).abs() < 1e-6);
        assert!((v["b"].as_f64().unwrap() - 2.0).abs() < 1e-6);
    }
}
