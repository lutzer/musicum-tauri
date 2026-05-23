use audio_plugin_sdk::{
    implement_plugin, AudioPlugin, FloatParam, PluginDescriptor, PluginMode, PluginParameter
};

static GAIN_PARAMS: [PluginParameter; 1] = [PluginParameter::Float {
    id: "gain",
    name: "Gain",
    min: 0.0,
    max: 4.0,
    default: 1.0,
    step: 0.01,
    unit: "x",
    disabled: false,
    hidden: false,
}];

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "gain",
    name: "Gain",
    version: "0.1.0",
    mode: PluginMode::Realtime,
    parameters: &GAIN_PARAMS,
};

pub struct GainPlugin {
    gain: FloatParam,
}

impl AudioPlugin for GainPlugin {
    fn descriptor() -> &'static PluginDescriptor {
        &DESCRIPTOR
    }

    fn new() -> Self {
        GainPlugin { gain: GAIN_PARAMS[0].float_param() }
    }

    fn set_parameter(&mut self, id: &str, value: f32) {
        if id == "gain" {
            self.gain.set(value);
        }
    }

    fn get_parameter(&self, id: &str) -> f32 {
        if id == "gain" { self.gain.get() } else { 0.0 }
    }

    fn process(
        &mut self,
        samples: &mut [f32],
        _channels: usize,
        _sample_rate: f32,
        _timestamp_secs: f64,
    ) {
        for s in samples.iter_mut() {
            *s *= self.gain.get();
        }
    }
}

implement_plugin!(GainPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use audio_plugin_sdk::AudioPlugin;

    #[test]
    fn gain_at_one_is_passthrough() {
        let mut plugin = GainPlugin::new();
        let mut samples = vec![0.5_f32, -0.5, 1.0, -1.0];
        let expected = samples.clone();
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        assert_eq!(samples, expected);
    }

    #[test]
    fn gain_at_zero_silences() {
        let mut plugin = GainPlugin::new();
        plugin.set_parameter("gain", 0.0);
        let mut samples = vec![0.5_f32, -0.5, 1.0, -1.0];
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        assert!(samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn gain_at_half_halves_amplitude() {
        let mut plugin = GainPlugin::new();
        plugin.set_parameter("gain", 0.5);
        let mut samples = vec![1.0_f32, -1.0, 0.5, -0.5];
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        let expected = vec![0.5_f32, -0.5, 0.25, -0.25];
        for (a, b) in samples.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn get_unknown_parameter_returns_zero() {
        let plugin = GainPlugin::new();
        assert_eq!(plugin.get_parameter("unknown"), 0.0);
    }

    #[test]
    fn descriptor_contains_gain_parameter() {
        let json = GainPlugin::descriptor().to_json();
        assert!(json.contains("\"id\":\"gain\""));
        assert!(json.contains("\"parameters\""));
        assert!(json.contains("\"type\":\"float\""));
    }
}
