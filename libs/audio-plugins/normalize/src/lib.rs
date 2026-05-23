use audio_plugin_sdk::{
    implement_plugin, implement_analyzer, AudioAnalyzer, AudioPlugin,
    BoolParam, FloatParam, ParamMap, ParamResult, PluginDescriptor, PluginMode, PluginParameter,
};

static NORMALIZE_PARAMS: [PluginParameter; 4] = [
    PluginParameter::Float {
        id: "target_dbfs",
        name: "Target dBFS",
        min: -40.0,
        max: 0.0,
        default: -3.0,
        step: 0.5,
        unit: "dBFS",
        disabled: false,
        hidden: false,
    },
    PluginParameter::Action { id: "analyze", name: "Compute", disabled: false },
    PluginParameter::Float {
        id: "__computed_gain",
        name: "Computed Gain",
        min: 0.0,
        max: 100.0,
        default: 1.0,
        step: 0.001,
        unit: "x",
        disabled: true,
        hidden: false,
    },
    PluginParameter::Bool {
        id: "__analyzed",
        name: "Analyzed",
        default: false,
        disabled: true,
        hidden: true,
    },
];

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "normalize",
    name: "Normalize",
    version: "1.0.0",
    mode: PluginMode::Analyzed,
    parameters: &NORMALIZE_PARAMS,
};

pub struct NormalizePlugin {
    /// User-controlled target ceiling in dBFS (e.g. -3.0).
    target_dbfs: FloatParam,
    /// Computed linear gain factor. Set by on_analysis_result(), also settable
    /// as a parameter so the frontend can forward it to the worklet instance.
    computed_gain: FloatParam,
    analyzed: BoolParam,
}

impl AudioPlugin for NormalizePlugin {
    fn descriptor() -> &'static PluginDescriptor {
        &DESCRIPTOR
    }

    fn new() -> Self {
        NormalizePlugin {
            target_dbfs: NORMALIZE_PARAMS[0].float_param(),
            computed_gain: NORMALIZE_PARAMS[2].float_param(),
            analyzed: NORMALIZE_PARAMS[3].bool_param(),
        }
    }

    fn set_parameter(&mut self, id: &str, value: f32) {
        match id {
            "target_dbfs" => self.target_dbfs.set(value),
            "__computed_gain" => self.computed_gain.set(value),
            "__analyzed" => self.analyzed.set(value),
            _ => {}
        }
    }

    fn get_parameter(&self, id: &str) -> f32 {
        match id {
            "target_dbfs" => self.target_dbfs.get(),
            "__computed_gain" => self.computed_gain.get(),
            "__analyzed" => self.analyzed.get(),
            _ => 0.0,
        }
    }

    fn process(
        &mut self,
        samples: &mut [f32],
        _channels: usize,
        _sample_rate: f32,
        _timestamp_secs: f64,
    ) {
        for s in samples.iter_mut() {
            *s *= self.computed_gain.get();
        }
    }
}

pub struct NormalizeAnalyzer {
    peak: f32,
    target_dbfs: f32
}

impl AudioAnalyzer for NormalizeAnalyzer {
    type Plugin = NormalizePlugin;

    fn new() -> Self {
        NormalizeAnalyzer { peak: 0.0, target_dbfs: -3.0 }
    }

    fn init(&mut self, params: ParamMap) {
        self.target_dbfs = params.get_float("target_dbfs");
    }

    fn analyze(&mut self, samples: &[f32], _channels: usize, _sample_rate: f32, _timestamp_secs: f64) {
        for &s in samples {
            let abs = s.abs();
            if abs > self.peak {
                self.peak = abs;
            }
        }
    }

    fn finish_analysis(&self) -> ParamResult {
        let target_linear = 10_f32.powf(self.target_dbfs / 20.0);
        ParamResult::new()
            .with("__computed_gain", target_linear / self.peak.max(f32::EPSILON))
    }
}

implement_plugin!(NormalizePlugin);
implement_analyzer!(NormalizeAnalyzer);

#[cfg(test)]
mod tests {
    use super::*;
    use audio_plugin_sdk::{AudioPlugin};


    // ── NormalizePlugin tests ────────────────────────────────────────────────

    #[test]
    fn computed_gain_settable_via_set_parameter() {
        let mut plugin = NormalizePlugin::new();
        plugin.set_parameter("__computed_gain", 2.5);
        assert!((plugin.get_parameter("__computed_gain") - 2.5).abs() < 1e-6);
        let mut samples = vec![0.4_f32];
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        assert!((samples[0] - 1.0).abs() < 1e-6);
    }
}
