use audio_plugin_sdk::{
    implement_plugin, AudioPlugin, FloatParam, PluginDescriptor, PluginMode, PluginParameter
};

static OSCILLOSCOPE_PARAMS: [PluginParameter; 3] = [
    PluginParameter::Float {
        id: "chunk_size",
        name: "Chunk Size",
        min: 32.0,
        max: 16348.0,
        default: 1024.0,
        step: 8.0,
        unit: "samples",
        disabled: false,
        hidden: false,
    },
    PluginParameter::Float {
        id: "trigger_threshold",
        name: "Trigger Threshold",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: 0.01,
        unit: "",
        disabled: false,
        hidden: false,
    },
    PluginParameter::Canvas {
        id: "waveform",
        name: "Waveform",
        aspect_ratio: 1.0,
        disabled: false
    },
];

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "oscilloscope",
    name: "Oscilloscope",
    version: "0.1.0",
    mode: PluginMode::Realtime,
    parameters: &OSCILLOSCOPE_PARAMS,
};

pub struct OscilloscopePlugin {
    chunk_size: FloatParam,
    trigger_threshold: FloatParam,
    /// Ring buffer of the latest chunk_size samples (interleaved channels).
    buffer: Vec<f32>,
}

impl OscilloscopePlugin {
    /// Number of samples currently in the accumulation buffer.
    /// Exposed for testing only.
    #[cfg(test)]
    pub fn sample_count(&self) -> usize {
        self.buffer.len()
    }
}

impl AudioPlugin for OscilloscopePlugin {
    fn descriptor() -> &'static PluginDescriptor {
        &DESCRIPTOR
    }

    fn new() -> Self {
        OscilloscopePlugin {
            chunk_size: OSCILLOSCOPE_PARAMS[0].float_param(),
            trigger_threshold: OSCILLOSCOPE_PARAMS[1].float_param(),
            buffer: Vec::new(),
        }
    }

    fn set_parameter(&mut self, id: &str, value: f32) {
        if id == "chunk_size" {
            self.chunk_size.set(value);
        } else if id == "trigger_threshold" {
            self.trigger_threshold.set(value);
        }
    }

    fn get_parameter(&self, id: &str) -> f32 {
        if id == "chunk_size" { 
            self.chunk_size.get() 
        } else if id == "trigger_threshold" {
            self.trigger_threshold.get()
        } else { 0.0 }
    }

    fn process(
        &mut self,
        samples: &mut [f32],
        _channels: usize,
        _sample_rate: f32,
        _timestamp_secs: f64,
    ) {
        let cap = self.chunk_size.get() as usize;
        self.buffer.extend_from_slice(samples);
        if self.buffer.len() > cap {
            let excess = self.buffer.len() - cap;
            self.buffer.drain(..excess);
        }
        // Pass-through: oscilloscope does not modify audio.
    }

    fn render_snapshot(&self) -> &[u8] {
        // Safety: f32 is plain data; alignment is guaranteed by Vec<f32>.
        unsafe {
            std::slice::from_raw_parts(
                self.buffer.as_ptr() as *const u8,
                self.buffer.len() * 4,
            )
        }
    }
}


implement_plugin!(OscilloscopePlugin);

#[cfg(test)]
mod tests {
    use super::*;
    use audio_plugin_sdk::AudioPlugin;

    #[test]
    fn process_is_passthrough() {
        let mut plugin = OscilloscopePlugin::new();
        let mut samples = vec![0.5_f32, -0.5, 1.0, -1.0];
        let expected = samples.clone();
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        assert_eq!(samples, expected);
    }

    #[test]
    fn chunk_size_parameter_roundtrip() {
        let mut plugin = OscilloscopePlugin::new();
        plugin.set_parameter("chunk_size", 2048.0);
        assert_eq!(plugin.get_parameter("chunk_size"), 2048.0);
    }

    #[test]
    fn get_unknown_parameter_returns_zero() {
        let plugin = OscilloscopePlugin::new();
        assert_eq!(plugin.get_parameter("unknown"), 0.0);
    }

    #[test]
    fn descriptor_contains_canvas_parameter() {
        let json = OscilloscopePlugin::descriptor().to_json();
        assert!(json.contains("\"type\":\"canvas\""));
        assert!(json.contains("\"id\":\"waveform\""));
        assert!(json.contains("\"aspect_ratio\":1.0"));
    }

    #[test]
    fn process_accumulates_samples_in_buffer() {
        let mut plugin = OscilloscopePlugin::new();
        let mut samples = vec![0.1_f32, 0.2, 0.3, 0.4];
        plugin.process(&mut samples, 2, 44100.0, 0.0);
        // Buffer should have 4 samples (2 frames × 2 channels).
        assert_eq!(plugin.sample_count(), 4);
    }

    #[test]
    fn buffer_caps_at_chunk_size() {
        let mut plugin = OscilloscopePlugin::new();
        // chunk_size min is 256; feed twice that so the cap kicks in.
        plugin.set_parameter("chunk_size", 256.0);
        let mut samples: Vec<f32> = (0..512).map(|i| i as f32).collect();
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        assert_eq!(plugin.sample_count(), 256);
    }

    #[test]
    fn render_snapshot_returns_bytes_matching_buffer() {
        let mut plugin = OscilloscopePlugin::new();
        let mut samples = vec![1.0_f32, -1.0];
        plugin.process(&mut samples, 1, 44100.0, 0.0);
        let snapshot = plugin.render_snapshot();
        // 2 f32 values = 8 bytes.
        assert_eq!(snapshot.len(), 8);
        // Reinterpret first 4 bytes as f32 — must equal 1.0.
        let val = f32::from_le_bytes(snapshot[..4].try_into().unwrap());
        assert!((val - 1.0_f32).abs() < 1e-6);
    }
}
