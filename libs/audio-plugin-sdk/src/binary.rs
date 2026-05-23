/// Generates a complete `main()` for a native plugin processing binary.
///
/// Usage: call `audio_plugin_sdk::implement_plugin_binary!(MyPlugin);` in
/// `src/ap_main.rs` of any plugin crate.
///
/// Binary contract:
///   ap-<plugin>  <input.wav>  <output.wav>
///   stdin: {"param_id": value, ...}
///
/// Reads input WAV, applies plugin.process(), writes output WAV as f32.
/// Exit 0 on success, non-zero on failure.
#[macro_export]
macro_rules! implement_plugin_binary {
    ($ty:ty) => {
        fn main() {
            use std::io::Read as _;
            use $crate::AudioPlugin as _;

            let args: Vec<String> = std::env::args().collect();
            if args.len() < 3 {
                eprintln!("Usage: {} <input.wav> <output.wav>", args[0]);
                std::process::exit(1);
            }
            let input_path = &args[1];
            let output_path = &args[2];

            // Read parameters from stdin.
            let mut stdin_json = String::new();
            std::io::stdin()
                .read_to_string(&mut stdin_json)
                .expect("failed to read stdin");

            // Parse parameters from JSON using descriptor defaults as fallback.
            let descriptor = <$ty as $crate::AudioPlugin>::descriptor();
            let param_map = descriptor.parse_params(&stdin_json);

            // Create plugin and set all Float/Bool parameters.
            let mut plugin = <$ty as $crate::AudioPlugin>::new();
            for param in descriptor.parameters {
                match param {
                    $crate::PluginParameter::Float { id, .. } => {
                        plugin.set_parameter(id, param_map.get_float(id));
                    }
                    $crate::PluginParameter::Bool { id, .. } => {
                        let v = if param_map.get_bool(id) { 1.0_f32 } else { 0.0 };
                        plugin.set_parameter(id, v);
                    }
                    _ => {}
                }
            }

            // Read input WAV into interleaved f32 samples.
            let mut reader =
                $crate::hound::WavReader::open(input_path).expect("failed to open input WAV");
            let spec = reader.spec();
            let channels = spec.channels as usize;
            let sample_rate = spec.sample_rate as f32;

            let mut samples: Vec<f32> = match spec.sample_format {
                $crate::hound::SampleFormat::Float => {
                    reader.samples::<f32>().map(|s| s.unwrap()).collect()
                }
                $crate::hound::SampleFormat::Int => {
                    let scale = (1i64 << (spec.bits_per_sample as u32 - 1)) as f32;
                    reader.samples::<i32>().map(|s| s.unwrap() as f32 / scale).collect()
                }
            };

            // Apply plugin DSP.
            plugin.process(&mut samples, channels, sample_rate, 0.0);

            // Write output WAV as 32-bit float.
            let out_spec = $crate::hound::WavSpec {
                channels: spec.channels,
                sample_rate: spec.sample_rate,
                bits_per_sample: 32,
                sample_format: $crate::hound::SampleFormat::Float,
            };
            let mut writer = $crate::hound::WavWriter::create(output_path, out_spec)
                .expect("failed to create output WAV");
            for &s in &samples {
                writer.write_sample(s).expect("failed to write sample");
            }
            writer.finalize().expect("failed to finalize WAV");
        }
    };
}
