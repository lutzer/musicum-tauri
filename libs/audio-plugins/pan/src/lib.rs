use audio_plugin_sdk::{
    implement_plugin, AudioPlugin, FloatParam, PluginDescriptor, PluginMode, PluginParameter
};

static PAN_PARAMS: [PluginParameter; 2] = [
    PluginParameter::Float {
        id: "pan",
        name: "Pan",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        unit: "",
        disabled: false,
        hidden: false,
    },
    PluginParameter::Float {
        id: "width",
        name: "Width",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        step: 0.01,
        unit: "",
        disabled: false,
        hidden: false,
    },
];

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "pan",
    name: "Pan / Width",
    version: "0.1.0",
    mode: PluginMode::Realtime,
    parameters: &PAN_PARAMS,
};

/// Pan and stereo width plugin.
///
/// Processing chain (applied per frame):
///   1. M/S width: expands or collapses the stereo field.
///      - width=0 → mono (side cancelled)
///      - width=1 → unchanged stereo
///      - width>1 → extra wide (side boosted)
///   2. Balance pan: attenuates one channel to move the image left or right.
///      - pan=0  → both channels at full level (passthrough)
///      - pan=-1 → right channel silenced, left at full
///      - pan=+1 → left channel silenced, right at full
///
/// For mono input (channels=1) width is ignored and pan attenuates the signal.
pub struct PanPlugin {
    pan: FloatParam,
    width: FloatParam,
}

impl AudioPlugin for PanPlugin {
    fn descriptor() -> &'static PluginDescriptor {
        &DESCRIPTOR
    }

    fn new() -> Self {
        PanPlugin {
            pan: PAN_PARAMS[0].float_param(),
            width: PAN_PARAMS[1].float_param(),
        }
    }

    fn set_parameter(&mut self, id: &str, value: f32) {
        match id {
            "pan" => self.pan.set(value),
            "width" => self.width.set(value),
            _ => {}
        }
    }

    fn get_parameter(&self, id: &str) -> f32 {
        match id {
            "pan" => self.pan.get(),
            "width" => self.width.get(),
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
        if channels < 2 {
            let gain = 1.0 - self.pan.get().abs();
            for s in samples.iter_mut() {
                *s *= gain;
            }
            return;
        }

        let left_gain = (1.0 - self.pan.get()).clamp(0.0, 1.0);
        let right_gain = (1.0 + self.pan.get()).clamp(0.0, 1.0);

        let frames = samples.len() / channels;
        for f in 0..frames {
            let l = samples[f * channels];
            let r = samples[f * channels + 1];

            let mid = (l + r) * 0.5;
            let side = (l - r) * 0.5 * self.width.get();

            samples[f * channels] = (mid + side) * left_gain;
            samples[f * channels + 1] = (mid - side) * right_gain;
        }
    }
}

implement_plugin!(PanPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use audio_plugin_sdk::AudioPlugin;

    fn process_stereo(plugin: &mut PanPlugin, l: f32, r: f32) -> (f32, f32) {
        let mut samples = vec![l, r];
        plugin.process(&mut samples, 2, 44100.0, 0.0);
        (samples[0], samples[1])
    }

    #[test]
    fn centre_pan_unity_width_is_passthrough() {
        let mut plugin = PanPlugin::new(); // pan=0, width=1
        let (out_l, out_r) = process_stereo(&mut plugin, 0.8, -0.4);
        assert!((out_l - 0.8).abs() < 1e-6);
        assert!((out_r - (-0.4)).abs() < 1e-6);
    }

    #[test]
    fn full_right_pan_silences_left() {
        let mut plugin = PanPlugin::new();
        plugin.set_parameter("pan", 1.0);
        plugin.set_parameter("width", 1.0);
        let (out_l, out_r) = process_stereo(&mut plugin, 0.5, 0.5);
        assert!(out_l.abs() < 1e-6, "left should be silent");
        assert!((out_r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn full_left_pan_silences_right() {
        let mut plugin = PanPlugin::new();
        plugin.set_parameter("pan", -1.0);
        plugin.set_parameter("width", 1.0);
        let (out_l, out_r) = process_stereo(&mut plugin, 0.5, 0.5);
        assert!((out_l - 0.5).abs() < 1e-6);
        assert!(out_r.abs() < 1e-6, "right should be silent");
    }

    #[test]
    fn zero_width_produces_mono() {
        let mut plugin = PanPlugin::new();
        plugin.set_parameter("width", 0.0);
        let (out_l, out_r) = process_stereo(&mut plugin, 0.6, 0.2);
        let expected = (0.6 + 0.2) * 0.5;
        assert!((out_l - expected).abs() < 1e-6, "L should be mid");
        assert!((out_r - expected).abs() < 1e-6, "R should be mid");
    }

    #[test]
    fn double_width_widens_stereo() {
        let mut plugin = PanPlugin::new();
        plugin.set_parameter("width", 2.0);
        let (out_l, out_r) = process_stereo(&mut plugin, 1.0, 0.0);
        // mid=(1+0)/2=0.5, side=(1-0)/2*2=1.0 → L=1.5, R=-0.5
        assert!((out_l - 1.5).abs() < 1e-6);
        assert!((out_r - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn mono_input_centre_pan_is_passthrough() {
        let mut plugin = PanPlugin::new();
        let mut samples = vec![0.7_f32, 0.3, -0.5];
        let expected = samples.clone();
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        for (a, b) in samples.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn mono_input_full_pan_silences() {
        let mut plugin = PanPlugin::new();
        plugin.set_parameter("pan", 1.0);
        let mut samples = vec![1.0_f32, -1.0];
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        assert!(samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn parameters_clamp() {
        let mut plugin = PanPlugin::new();
        plugin.set_parameter("pan", 99.0);
        assert_eq!(plugin.get_parameter("pan"), 1.0);
        plugin.set_parameter("pan", -99.0);
        assert_eq!(plugin.get_parameter("pan"), -1.0);
        plugin.set_parameter("width", -1.0);
        assert_eq!(plugin.get_parameter("width"), 0.0);
        plugin.set_parameter("width", 99.0);
        assert_eq!(plugin.get_parameter("width"), 2.0);
    }

    #[test]
    fn unknown_parameter_returns_zero() {
        let plugin = PanPlugin::new();
        assert_eq!(plugin.get_parameter("unknown"), 0.0);
    }
}
