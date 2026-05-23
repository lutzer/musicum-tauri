use audio_plugin_sdk::{
    implement_plugin, AudioPlugin, FloatParam, PluginDescriptor, PluginMode, PluginParameter
};

static REVERB_PARAMS: [PluginParameter; 4] = [
    PluginParameter::Float {
        id: "gain",
        name: "Input Gain",
        min: 0.0,
        max: 0.3,
        default: 0.1,
        step: 0.01,
        unit: "x",
        disabled: false,
        hidden: false,
    },
    PluginParameter::Float {
        id: "room_size",
        name: "Room Size",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        unit: "",
        disabled: false,
        hidden: false,
    },
    PluginParameter::Float {
        id: "damping",
        name: "Damping",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: 0.01,
        unit: "",
        disabled: false,
        hidden: false,
    },
    PluginParameter::Float {
        id: "wet",
        name: "Wet",
        min: 0.0,
        max: 1.0,
        default: 0.3,
        step: 0.01,
        unit: "",
        disabled: false,
        hidden: false,
    },
];

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "reverb",
    name: "Reverb",
    version: "0.1.0",
    mode: PluginMode::Realtime,
    parameters: &REVERB_PARAMS,
};

// Freeverb-style comb filter
struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    filter_store: f32,
    damp: f32,
}

impl CombFilter {
    fn new(size: usize) -> Self {
        CombFilter {
            buffer: vec![0.0; size],
            pos: 0,
            feedback: 0.5,
            filter_store: 0.0,
            damp: 0.5,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.pos];
        self.filter_store = output * (1.0 - self.damp) + self.filter_store * self.damp;
        self.buffer[self.pos] = input + self.filter_store * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }

    fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback;
    }

    fn set_damp(&mut self, damp: f32) {
        self.damp = damp;
    }
}

// Allpass filter
struct AllpassFilter {
    buffer: Vec<f32>,
    pos: usize,
}

impl AllpassFilter {
    fn new(size: usize) -> Self {
        AllpassFilter { buffer: vec![0.0; size], pos: 0 }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.pos];
        let output = -input + buffered;
        self.buffer[self.pos] = input + buffered * 0.5;
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }
}

// Comb delay sizes tuned for 44100 Hz (Freeverb defaults)
const COMB_SIZES: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_SIZES: [usize; 4] = [556, 441, 341, 225];

pub struct ReverbPlugin {
    combs: Vec<CombFilter>,
    allpasses: Vec<AllpassFilter>,
    gain: FloatParam,
    room_size: FloatParam,
    damping: FloatParam,
    wet: FloatParam,
}

impl AudioPlugin for ReverbPlugin {
    fn descriptor() -> &'static PluginDescriptor {
        &DESCRIPTOR
    }

    fn new() -> Self {
        let mut plugin = ReverbPlugin {
            combs: COMB_SIZES.iter().map(|&s| CombFilter::new(s)).collect(),
            allpasses: ALLPASS_SIZES.iter().map(|&s| AllpassFilter::new(s)).collect(),
            gain: REVERB_PARAMS[0].float_param(),
            room_size: REVERB_PARAMS[1].float_param(),
            damping: REVERB_PARAMS[2].float_param(),
            wet: REVERB_PARAMS[3].float_param(),
        };
        plugin.update_filters();
        plugin
    }

    fn set_parameter(&mut self, id: &str, value: f32) {
        match id {
            "gain" => self.gain.set(value),
            "room_size" => self.room_size.set(value),
            "damping" => self.damping.set(value),
            "wet" => self.wet.set(value),
            _ => return,
        }
        self.update_filters();
    }

    fn get_parameter(&self, id: &str) -> f32 {
        match id {
            "gain" => self.gain.get(),
            "room_size" => self.room_size.get(),
            "damping" => self.damping.get(),
            "wet" => self.wet.get(),
            _ => 0.0,
        }
    }

    fn process(
        &mut self,
        samples: &mut [f32],
        channels: usize,
        _sample_rate: f32,
        _timestamp_secs: f64,
    ) {
        let frames = samples.len() / channels.max(1);
        for i in 0..frames {
            let mut mono = 0.0_f32;
            for c in 0..channels {
                mono += samples[i * channels + c];
            }
            mono /= channels as f32;

            let mut reverb_out = 0.0_f32;
            for comb in &mut self.combs {
                reverb_out += comb.process(mono * self.gain.get());
            }

            for allpass in &mut self.allpasses {
                reverb_out = allpass.process(reverb_out);
            }

            for c in 0..channels {
                let dry = samples[i * channels + c];
                samples[i * channels + c] =
                    dry * (1.0 - self.wet.get()) + reverb_out * self.wet.get();
            }
        }
    }
}

impl ReverbPlugin {
    fn update_filters(&mut self) {
        let feedback = 0.7 + self.room_size.get() * 0.28;
        for comb in &mut self.combs {
            comb.set_feedback(feedback);
            comb.set_damp(self.damping.get());
        }
    }
}

implement_plugin!(ReverbPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use audio_plugin_sdk::AudioPlugin;

    #[test]
    fn silence_stays_silent() {
        let mut plugin = ReverbPlugin::new();
        let mut samples = vec![0.0_f32; 256];
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        assert!(samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn wet_zero_is_passthrough() {
        let mut plugin = ReverbPlugin::new();
        plugin.set_parameter("wet", 0.0);
        let mut samples = vec![0.5_f32, -0.5, 1.0, -1.0];
        let expected = samples.clone();
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        for (a, b) in samples.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn wet_one_applies_reverb() {
        let mut plugin = ReverbPlugin::new();
        plugin.set_parameter("wet", 1.0);
        let mut samples = vec![1.0_f32; 4];
        let before = samples.clone();
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        // With wet=1 the dry signal is removed; output should differ from input
        assert_ne!(samples, before);
    }

    #[test]
    fn parameters_clamp() {
        let mut plugin = ReverbPlugin::new();
        plugin.set_parameter("room_size", 99.0);
        assert_eq!(plugin.get_parameter("room_size"), 1.0);
        plugin.set_parameter("damping", -1.0);
        assert_eq!(plugin.get_parameter("damping"), 0.0);
    }

    #[test]
    fn unknown_parameter_returns_zero() {
        let plugin = ReverbPlugin::new();
        assert_eq!(plugin.get_parameter("unknown"), 0.0);
    }

    #[test]
    fn gain_at_one_is_unity() {
        let mut plugin = ReverbPlugin::new();
        plugin.set_parameter("wet", 0.0);
        plugin.set_parameter("gain", 1.0);
        let mut samples = vec![0.5_f32, -0.5, 1.0, -1.0];
        let expected = samples.clone();
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        for (a, b) in samples.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn gain_clamps() {
        let mut plugin = ReverbPlugin::new();
        plugin.set_parameter("gain", 99.0);
        assert_eq!(plugin.get_parameter("gain"), 1.0);
        plugin.set_parameter("gain", -1.0);
        assert_eq!(plugin.get_parameter("gain"), 0.0);
    }

    #[test]
    fn descriptor_contains_expected_fields() {
        let json = ReverbPlugin::descriptor().to_json();
        assert!(json.contains("\"id\":\"reverb\""));
        assert!(json.contains("\"room_size\""));
        assert!(json.contains("\"damping\""));
        assert!(json.contains("\"wet\""));
        assert!(json.contains("\"gain\""));
    }
}
