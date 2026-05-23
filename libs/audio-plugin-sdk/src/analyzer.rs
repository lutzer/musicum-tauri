/// Trait for whole-file audio analyzers.
///
/// An analyzer processes audio sample-by-sample (via repeated [`analyze`](Self::analyze)
/// calls) and produces a typed result via [`finish_analysis`](Self::finish_analysis).
///
/// Use [`implement_analyzer!`] to generate the required `__aa_*` C ABI exports.
/// The frontend instantiates the same WASM binary used by the plugin but calls
/// the `__aa_*` exports on the main thread instead of the AudioWorklet.
pub trait AudioAnalyzer: Sized {
    /// The plugin type this analyzer corresponds to.
    ///
    /// The `implement_analyzer!` macro uses `Self::Plugin::descriptor()` to
    /// build the [`ParamMap`](crate::ParamMap) before calling [`init`](Self::init), so the
    /// analyzer never handles raw JSON.
    type Plugin: crate::AudioPlugin;

    /// Create a new instance with default state.
    fn new() -> Self;

    /// Receive the plugin's current parameter values before analysis begins.
    ///
    /// Called once after `__aa_create` / `__aa_reset`, before any `__aa_analyze`
    /// calls. Default: no-op — analyzers that need no parameters can skip this.
    fn init(&mut self, _params: crate::ParamMap) {}

    /// Process one chunk of interleaved audio samples.
    ///
    /// `samples` is an interleaved f32 buffer: `[L0, R0, L1, R1, …]`.
    /// `timestamp_secs` is the track-relative position of the first sample.
    /// Called repeatedly until the full file has been fed.
    fn analyze(&mut self, samples: &[f32], channels: usize, sample_rate: f32, timestamp_secs: f64);

    /// Finalize and return the analysis result.
    ///
    /// The plugin receives this via `on_analysis_result`. Called once by the
    /// ABI; the result is cached as JSON.
    fn finish_analysis(&self) -> crate::ParamResult;
}

/// Generate the full C ABI required by the Musicum analyzer runtime for a type
/// that implements [`AudioAnalyzer`].
///
/// # Usage
///
/// ```rust,ignore
/// use audio_plugin_sdk::{AudioAnalyzer, implement_analyzer};
///
/// struct MyAnalyzer { /* ... */ }
/// impl AudioAnalyzer for MyAnalyzer { /* ... */ }
///
/// implement_analyzer!(MyAnalyzer);
/// ```
#[macro_export]
macro_rules! implement_analyzer {
    ($t:ty) => {
        static mut __AA_INSTANCE: Option<$t> = None;
        static mut __AA_RESULT: Option<String> = None;

        mod __analyzer_exports {
            #![allow(static_mut_refs)]
            use super::*;

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_alloc(size: u32) -> u32 {
                let mut buf = Vec::<u8>::with_capacity(size as usize);
                let ptr = buf.as_mut_ptr() as u32;
                std::mem::forget(buf);
                ptr
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_free(ptr: u32, len: u32) {
                unsafe {
                    drop(Vec::from_raw_parts(
                        ptr as *mut u8,
                        len as usize,
                        len as usize,
                    ));
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_create() {
                unsafe {
                    __AA_INSTANCE = Some(<$t as $crate::AudioAnalyzer>::new());
                    __AA_RESULT = None;
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_reset() {
                unsafe {
                    __AA_INSTANCE = Some(<$t as $crate::AudioAnalyzer>::new());
                    __AA_RESULT = None;
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_init(ptr: u32, len: u32) {
                unsafe {
                    if let Some(instance) = __AA_INSTANCE.as_mut() {
                        let bytes = std::slice::from_raw_parts(ptr as *const u8, len as usize);
                        if let Ok(s) = std::str::from_utf8(bytes) {
                            let params = <$t as $crate::AudioAnalyzer>::Plugin::descriptor()
                                .parse_params(s);
                            instance.init(params);
                        }
                    }
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_analyze(
                ptr: u32,
                len: u32,
                channels: u32,
                sample_rate: f32,
                timestamp_secs: f64,
            ) {
                unsafe {
                    if let Some(instance) = __AA_INSTANCE.as_mut() {
                        let samples =
                            std::slice::from_raw_parts(ptr as *const f32, len as usize);
                        instance.analyze(samples, channels as usize, sample_rate, timestamp_secs);
                    }
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_result_ptr() -> u32 {
                unsafe {
                    if __AA_INSTANCE.is_none() {
                        return 0;
                    }
                    if __AA_RESULT.is_none() {
                        __AA_RESULT = Some(
                            __AA_INSTANCE.as_ref().unwrap().finish_analysis().to_json()
                        );
                    }
                    __AA_RESULT.as_ref().map_or(0, |s| s.as_ptr() as u32)
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __aa_result_len() -> usize {
                unsafe {
                    if __AA_INSTANCE.is_none() {
                        return 0;
                    }
                    if __AA_RESULT.is_none() {
                        __AA_RESULT = Some(
                            __AA_INSTANCE.as_ref().unwrap().finish_analysis().to_json()
                        );
                    }
                    __AA_RESULT.as_ref().unwrap().len()
                }
            }
        }
    };
}
