use audio_plugin_sdk::{
    implement_plugin, AudioPlugin, PluginDescriptor, PluginMode, PluginParameter,
};

static LEVEL_METER_PARAMS: [PluginParameter; 2] = [
    PluginParameter::Canvas {
        id: "level_left",
        name: "Left",
        aspect_ratio: 10.0,
        disabled: false
    },
    PluginParameter::Canvas {
        id: "level_right",
        name: "Right",
        aspect_ratio: 10.0,
        disabled: false
    },
];

static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
    id: "level-meter",
    name: "Level Meter",
    version: "0.1.0",
    mode: PluginMode::Realtime,
    parameters: &LEVEL_METER_PARAMS,
};

pub struct LevelMeter {
    left_peak: f32,
    right_peak: f32,
    left_hold: f32,
    right_hold: f32,
    left_hold_time: f64,
    right_hold_time: f64,
    /// Pre-packed snapshot bytes; updated at end of every process() call.
    snapshot_buf: [f32; 4],
}

impl AudioPlugin for LevelMeter {
    fn descriptor() -> &'static PluginDescriptor {
        &DESCRIPTOR
    }

    fn new() -> Self {
        LevelMeter {
            left_peak: 0.0,
            right_peak: 0.0,
            left_hold: 0.0,
            right_hold: 0.0,
            left_hold_time: 0.0,
            right_hold_time: 0.0,
            snapshot_buf: [0.0; 4],
        }
    }

    fn set_parameter(&mut self, _id: &str, _value: f32) {}

    fn get_parameter(&self, _id: &str) -> f32 {
        0.0
    }

    fn process(
        &mut self,
        samples: &mut [f32],
        _channels: usize,
        _sample_rate: f32,
        timestamp_secs: f64,
    ) {
        const HOLD_DURATION: f64 = 1.5;

        let mut left_peak = 0.0_f32;
        let mut right_peak = 0.0_f32;

        for (i, &s) in samples.iter().enumerate() {
            let abs = s.abs();
            if i % 2 == 0 {
                left_peak = left_peak.max(abs);
            } else {
                right_peak = right_peak.max(abs);
            }
        }

        self.left_peak = left_peak;
        self.right_peak = right_peak;

        // Update left hold
        if left_peak >= self.left_hold {
            self.left_hold = left_peak;
            self.left_hold_time = timestamp_secs;
        } else if timestamp_secs - self.left_hold_time > HOLD_DURATION {
            self.left_hold = left_peak;
        }

        // Update right hold
        if right_peak >= self.right_hold {
            self.right_hold = right_peak;
            self.right_hold_time = timestamp_secs;
        } else if timestamp_secs - self.right_hold_time > HOLD_DURATION {
            self.right_hold = right_peak;
        }

        self.snapshot_buf = [self.left_peak, self.right_peak, self.left_hold, self.right_hold];
    }

    fn render_snapshot(&self) -> &[u8] {
        // Safety: f32 is plain data; slice covers the exact 16 bytes of snapshot_buf.
        unsafe {
            std::slice::from_raw_parts(
                self.snapshot_buf.as_ptr() as *const u8,
                16,
            )
        }
    }
}

implement_plugin!(LevelMeter);

#[cfg(test)]
mod tests {
    use super::*;
    use audio_plugin_sdk::AudioPlugin;

    // ── descriptor ─────────────────────────────────────────────────────────

    #[test]
    fn descriptor_id_and_mode() {
        let json = LevelMeter::descriptor().to_json();
        assert!(json.contains("\"id\":\"level-meter\""));
        assert!(json.contains("\"mode\":\"realtime\""));
    }

    #[test]
    fn descriptor_has_two_canvas_params() {
        let json = LevelMeter::descriptor().to_json();
        assert!(json.contains("\"id\":\"level_left\""));
        assert!(json.contains("\"id\":\"level_right\""));
        assert!(json.contains("\"aspect_ratio\":10.0"));
    }

    // ── process – peak tracking ─────────────────────────────────────────────

    #[test]
    fn peaks_track_absolute_max_per_channel() {
        let mut p = LevelMeter::new();
        // Stereo interleaved: L=-0.8, R=0.3, L=0.5, R=-0.9
        let mut samples = vec![-0.8_f32, 0.3, 0.5, -0.9];
        p.process(&mut samples, 2, 44100.0, 0.0);
        let snap = parse_snapshot(&p);
        assert!((snap[0] - 0.8).abs() < 1e-6, "left peak");
        assert!((snap[1] - 0.9).abs() < 1e-6, "right peak");
    }

    #[test]
    fn process_is_passthrough() {
        let mut p = LevelMeter::new();
        let mut samples = vec![0.5_f32, -0.3, 0.2, 0.8];
        let expected = samples.clone();
        p.process(&mut samples, 2, 44100.0, 0.0);
        assert_eq!(samples, expected, "level meter must not modify audio");
    }

    #[test]
    fn hold_set_when_peak_exceeds_previous_hold() {
        let mut p = LevelMeter::new();
        // First buffer: left peak = 0.5
        let mut s = vec![0.5_f32, 0.0];
        p.process(&mut s, 2, 44100.0, 0.0);
        let snap1 = parse_snapshot(&p);
        assert!((snap1[2] - 0.5).abs() < 1e-6, "left hold should be 0.5");

        // Second buffer (same timestamp): left peak = 0.3 — hold stays at 0.5
        let mut s2 = vec![0.3_f32, 0.0];
        p.process(&mut s2, 2, 44100.0, 0.0);
        let snap2 = parse_snapshot(&p);
        assert!((snap2[2] - 0.5).abs() < 1e-6, "hold should not drop");
    }

    #[test]
    fn hold_decays_after_1_5_seconds() {
        let mut p = LevelMeter::new();
        // Establish hold at t=0
        let mut s = vec![0.8_f32, 0.0];
        p.process(&mut s, 2, 44100.0, 0.0);

        // 2 seconds later: peak drops to 0.1 — hold should follow current peak
        let mut s2 = vec![0.1_f32, 0.0];
        p.process(&mut s2, 2, 44100.0, 2.0);
        let snap = parse_snapshot(&p);
        assert!((snap[2] - 0.1).abs() < 1e-6, "hold should decay to current peak after 1.5s");
    }

    #[test]
    fn hold_does_not_decay_before_1_5_seconds() {
        let mut p = LevelMeter::new();
        let mut s = vec![0.8_f32, 0.0];
        p.process(&mut s, 2, 44100.0, 0.0);

        // 1 second later — hold must still be 0.8
        let mut s2 = vec![0.1_f32, 0.0];
        p.process(&mut s2, 2, 44100.0, 1.0);
        let snap = parse_snapshot(&p);
        assert!((snap[2] - 0.8).abs() < 1e-6, "hold must not decay before 1.5s");
    }

    #[test]
    fn snapshot_returns_16_bytes() {
        let mut p = LevelMeter::new();
        let mut s = vec![0.5_f32, 0.5];
        p.process(&mut s, 2, 44100.0, 0.0);
        assert_eq!(p.render_snapshot().len(), 16);
    }

    #[test]
    fn snapshot_order_is_left_peak_right_peak_left_hold_right_hold() {
        let mut p = LevelMeter::new();
        // L=0.6, R=0.4
        let mut s = vec![0.6_f32, 0.4];
        p.process(&mut s, 2, 44100.0, 0.0);
        let snap = parse_snapshot(&p);
        // snap[0]=left_peak, snap[1]=right_peak, snap[2]=left_hold, snap[3]=right_hold
        assert!((snap[0] - 0.6).abs() < 1e-6);
        assert!((snap[1] - 0.4).abs() < 1e-6);
        assert!((snap[2] - 0.6).abs() < 1e-6);
        assert!((snap[3] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn get_unknown_parameter_returns_zero() {
        let p = LevelMeter::new();
        assert_eq!(p.get_parameter("nonexistent"), 0.0);
    }

    // ── helpers ─────────────────────────────────────────────────────────────

    /// Parse the 16-byte render_snapshot into [left_peak, right_peak, left_hold, right_hold].
    fn parse_snapshot(p: &LevelMeter) -> [f32; 4] {
        let bytes = p.render_snapshot();
        assert_eq!(bytes.len(), 16);
        [
            f32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            f32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            f32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            f32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        ]
    }
}
