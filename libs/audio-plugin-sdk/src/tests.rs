#[cfg(test)]
mod tests {
    use crate::{
        AudioAnalyzer, AudioPlugin, BoolParam, FloatParam,
        ParamMap, ParamResult, PluginDescriptor, PluginMode, PluginParameter,
    };

    // -------------------------------------------------------------------------
    // Mock types
    // -------------------------------------------------------------------------

    static MOCK_PLUGIN_DESCRIPTOR: PluginDescriptor = PluginDescriptor {
        id: "mock-plugin",
        name: "Mock Plugin",
        version: "0.1.0",
        mode: PluginMode::Realtime,
        parameters: &[
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
        ],
    };

    struct MockPlugin {
        gain: FloatParam,
        active: BoolParam,
    }

    impl AudioPlugin for MockPlugin {
        fn descriptor() -> &'static PluginDescriptor {
            &MOCK_PLUGIN_DESCRIPTOR
        }
        fn new() -> Self {
            Self {
                gain: FloatParam::new(1.0, 0.0, 2.0),
                active: BoolParam::new(true),
            }
        }
        fn set_parameter(&mut self, id: &str, value: f32) {
            match id {
                "gain" => self.gain.set(value),
                "active" => self.active.set(value),
                _ => {}
            }
        }
        fn get_parameter(&self, id: &str) -> f32 {
            match id {
                "gain" => self.gain.get(),
                "active" => self.active.get(),
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
            if self.active.get_bool() {
                for s in samples.iter_mut() {
                    *s *= self.gain.get();
                }
            }
        }
    }

    struct MockAnalyzer {
        count: usize,
    }

    impl AudioAnalyzer for MockAnalyzer {
        type Plugin = MockPlugin;

        fn new() -> Self {
            Self { count: 0 }
        }
        fn analyze(
            &mut self,
            samples: &[f32],
            _channels: usize,
            _sample_rate: f32,
            _timestamp_secs: f64,
        ) {
            self.count += samples.len();
        }
        fn finish_analysis(&self) -> ParamResult {
            ParamResult::new().with("count", self.count as f32)
        }
    }

    // -------------------------------------------------------------------------
    // FloatParam tests
    // -------------------------------------------------------------------------

    #[test]
    fn float_param_get_returns_default() {
        let p = FloatParam::new(1.5, 0.0, 2.0);
        assert!((p.get() - 1.5).abs() < 1e-6);
    }

    #[test]
    fn float_param_set_roundtrips() {
        let mut p = FloatParam::new(1.0, 0.0, 2.0);
        p.set(0.75);
        assert!((p.get() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn float_param_set_clamps_below_min() {
        let mut p = FloatParam::new(1.0, 0.0, 2.0);
        p.set(-5.0);
        assert!((p.get() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn float_param_set_clamps_above_max() {
        let mut p = FloatParam::new(1.0, 0.0, 2.0);
        p.set(99.0);
        assert!((p.get() - 2.0).abs() < 1e-6);
    }

    // -------------------------------------------------------------------------
    // BoolParam tests
    // -------------------------------------------------------------------------

    #[test]
    fn bool_param_get_returns_float() {
        let f = BoolParam::new(false);
        assert!((f.get() - 0.0).abs() < 1e-6);
        let t = BoolParam::new(true);
        assert!((t.get() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bool_param_get_bool() {
        let f = BoolParam::new(false);
        assert!(!f.get_bool());
        let t = BoolParam::new(true);
        assert!(t.get_bool());
    }

    #[test]
    fn bool_param_set_nonzero_is_true() {
        let mut p = BoolParam::new(false);
        p.set(0.5);
        assert!(p.get_bool());
        p.set(0.0);
        assert!(!p.get_bool());
    }

    // -------------------------------------------------------------------------
    // PluginParameter helper tests
    // -------------------------------------------------------------------------

    #[test]
    fn plugin_parameter_float_param_ok() {
        let pp = PluginParameter::Float {
            id: "x",
            name: "X",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            unit: "",
            disabled: false,
            hidden: false,
        };
        let fp = pp.float_param();
        assert!((fp.get() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn plugin_parameter_bool_param_ok() {
        let pp = PluginParameter::Bool {
            id: "b",
            name: "B",
            default: true,
            disabled: false,
            hidden: false,
        };
        let bp = pp.bool_param();
        assert!(bp.get_bool());
    }

    #[test]
    #[should_panic]
    fn plugin_parameter_float_param_on_bool_panics() {
        let pp = PluginParameter::Bool {
            id: "b",
            name: "B",
            default: true,
            disabled: false,
            hidden: false,
        };
        let _ = pp.float_param();
    }

    #[test]
    #[should_panic]
    fn plugin_parameter_bool_param_on_float_panics() {
        let pp = PluginParameter::Float {
            id: "x",
            name: "X",
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: 0.01,
            unit: "",
            disabled: false,
            hidden: false,
        };
        let _ = pp.bool_param();
    }

    // -------------------------------------------------------------------------
    // PluginDescriptor tests
    // -------------------------------------------------------------------------

    #[test]
    fn descriptor_to_json_is_valid() {
        let json = MOCK_PLUGIN_DESCRIPTOR.to_json();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"mode\""));
        assert!(json.contains("\"parameters\""));
    }

    #[test]
    fn descriptor_mode_serializes_lowercase() {
        let realtime = serde_json::to_string(&PluginMode::Realtime).unwrap();
        assert_eq!(realtime, "\"realtime\"");
        let analyzed = serde_json::to_string(&PluginMode::Analyzed).unwrap();
        assert_eq!(analyzed, "\"analyzed\"");
        let offline = serde_json::to_string(&PluginMode::Offline).unwrap();
        assert_eq!(offline, "\"offline\"");
    }

    // -------------------------------------------------------------------------
    // MockPlugin trait behaviour
    // -------------------------------------------------------------------------

    #[test]
    fn mock_plugin_set_get_parameter_roundtrip() {
        let mut p = MockPlugin::new();
        p.set_parameter("gain", 1.8);
        assert!((p.get_parameter("gain") - 1.8).abs() < 1e-6);
    }

    #[test]
    fn mock_plugin_unknown_parameter_ignored() {
        let mut p = MockPlugin::new();
        p.set_parameter("unknown", 99.0);
        assert!((p.get_parameter("unknown") - 0.0).abs() < 1e-6);
    }

    #[test]
    fn mock_plugin_process_with_active_true_scales() {
        let mut p = MockPlugin::new();
        p.set_parameter("gain", 2.0);
        let mut samples = vec![0.5_f32, 0.5, 0.5];
        p.process(&mut samples, 1, 44100.0, 0.0);
        for s in &samples {
            assert!((s - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn mock_plugin_process_with_active_false_noop() {
        let mut p = MockPlugin::new();
        p.set_parameter("gain", 2.0);
        p.set_parameter("active", 0.0);
        let mut samples = vec![0.5_f32, 0.5, 0.5];
        p.process(&mut samples, 1, 44100.0, 0.0);
        for s in &samples {
            assert!((s - 0.5).abs() < 1e-6);
        }
    }

    // -------------------------------------------------------------------------
    // MockAnalyzer trait behaviour
    // -------------------------------------------------------------------------

    #[test]
    fn mock_analyzer_fresh_count_is_zero() {
        let a = MockAnalyzer::new();
        let json = a.finish_analysis().to_json();
        assert!(json.contains("\"count\":0"));
    }

    #[test]
    fn mock_analyzer_accumulates_sample_count() {
        let mut a = MockAnalyzer::new();
        a.analyze(&[1.0, 2.0, 3.0, 4.0], 1, 44100.0, 0.0);
        a.analyze(&[5.0, 6.0], 1, 44100.0, 0.0);
        let json = a.finish_analysis().to_json();
        assert!(json.contains("\"count\":6"));
    }

    #[test]
    fn mock_analyzer_init_does_not_panic() {
        let mut a = MockAnalyzer::new();
        let params = MockPlugin::descriptor().parse_params(r#"{"count":0}"#);
        a.init(params);
    }

    // -------------------------------------------------------------------------
    // Layer 2: ABI tests (WASM32 only)
    // -------------------------------------------------------------------------

    #[cfg(target_arch = "wasm32")]
    mod abi_tests {
        use super::*;
        use crate::{implement_analyzer, implement_plugin};

        mod plugin_abi {
            use super::*;
            use crate::implement_plugin;
            implement_plugin!(MockPlugin);
            use self::__plugin_exports::*;

            fn write_bytes(data: &[u8]) -> (u32, u32) {
                let ptr = __alloc(data.len() as u32);
                unsafe {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
                }
                (ptr, data.len() as u32)
            }

            fn write_str(s: &str) -> (u32, u32) {
                write_bytes(s.as_bytes())
            }

            #[test]
            fn abi_plugin_descriptor_is_valid_json() {
                let len = __ap_descriptor_len();
                assert_ne!(len, 0);
                let ptr = __ap_descriptor_ptr();
                assert_ne!(ptr, 0);
                let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
                let json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                assert_eq!(json["id"], "mock-plugin");
            }

            #[test]
            fn abi_plugin_new_drop_no_panic() {
                __ap_new();
                __ap_drop();
            }

            #[test]
            fn abi_plugin_set_get_parameter_roundtrip() {
                __ap_new();
                let (ptr, len) = write_str("gain");
                __ap_set_parameter(ptr, len, 1.7);
                let val = __ap_get_parameter(ptr, len);
                assert!((val - 1.7).abs() < 1e-6);
                __ap_drop();
            }

            #[test]
            fn abi_plugin_process_scales_samples() {
                __ap_new();
                let (gptr, glen) = write_str("gain");
                __ap_set_parameter(gptr, glen, 2.0);

                let mut samples: Vec<f32> = vec![0.5, 0.5, 0.5, 0.5];
                let buf_ptr = samples.as_mut_ptr() as u32;
                let buf_len = samples.len() as u32;
                __ap_process(buf_ptr, buf_len, 2, 44100.0, 0.0);
                for s in &samples {
                    assert!((s - 1.0).abs() < 1e-6);
                }
                __ap_drop();
            }

            #[test]
            fn abi_plugin_render_snapshot_default_empty() {
                __ap_new();
                let packed = __ap_render_snapshot();
                let len = (packed & 0xFFFF_FFFF) as usize;
                assert_eq!(len, 0);
                __ap_drop();
            }
        }

        mod analyzer_abi {
            use super::*;
            use crate::implement_analyzer;
            implement_analyzer!(MockAnalyzer);
            use self::__analyzer_exports::*;

            fn write_bytes(data: &[u8]) -> (u32, u32) {
                let ptr = __aa_alloc(data.len() as u32);
                unsafe {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
                }
                (ptr, data.len() as u32)
            }

            fn write_str(s: &str) -> (u32, u32) {
                write_bytes(s.as_bytes())
            }

            // All tests in one function to avoid static mut data races
            #[test]
            fn analyzer_abi_sequential() {
                // 1. create
                __aa_create();

                // 2. init with JSON
                let (ip, il) = write_str(r#"{"count":0}"#);
                __aa_init(ip, il);

                // 3. analyze 4 f32 samples
                let samples: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
                let buf_ptr = samples.as_ptr() as u32;
                let buf_len = (samples.len() * 4) as u32;
                __aa_analyze(buf_ptr, buf_len, 1, 44100.0, 0.0);

                // 4. result returns {"count":4}
                let packed = __aa_result();
                assert_ne!(packed, 0);
                let ptr = (packed >> 32) as *const u8;
                let len = (packed & 0xFFFF_FFFF) as usize;
                let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
                let json: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                assert_eq!(json["count"], 4);

                // 5. second call returns same ptr/len (cached)
                let packed2 = __aa_result();
                assert_eq!(packed, packed2);

                // 6. reset then result → count=0
                __aa_reset();
                let packed3 = __aa_result();
                assert_ne!(packed3, 0);
                let ptr3 = (packed3 >> 32) as *const u8;
                let len3 = (packed3 & 0xFFFF_FFFF) as usize;
                let bytes3 = unsafe { std::slice::from_raw_parts(ptr3, len3) };
                let json3: serde_json::Value = serde_json::from_slice(bytes3).unwrap();
                assert_eq!(json3["count"], 0);
            }
        }
    }
}
