use crate::parameters::PluginDescriptor;

/// Trait that every audio plugin must implement.
///
/// Override only the methods your plugin needs:
/// - Real-time effects (gain, reverb, …): override [`process`](Self::process).
/// - Plugins that trigger analysis: override [`trigger`](Self::trigger) to
///   emit an `"analyze"` event, and [`receive_data`](Self::receive_data) to
///   consume the analysis result.
///
/// Use [`implement_plugin!`] to generate the required C ABI exports.
/// The descriptor's [`PluginMode`](crate::PluginMode) tells the frontend which
/// methods are active.
pub trait AudioPlugin: Sized {
    /// Return the static descriptor for this plugin type.
    fn descriptor() -> &'static PluginDescriptor;

    /// Create a new instance with default parameter values.
    fn new() -> Self;

    /// Set a parameter by string ID. Unknown IDs are silently ignored.
    fn set_parameter(&mut self, id: &str, value: f32);

    /// Get a parameter by string ID. Returns `0.0` for unknown IDs.
    fn get_parameter(&self, id: &str) -> f32;

    /// Process `samples` in-place (real-time path).
    ///
    /// `samples` is an interleaved f32 buffer with `channels` channels at
    /// `sample_rate` Hz. Length is always a multiple of `channels`.
    /// `timestamp_secs` is the track-relative position of the first sample
    /// (seconds from track start, f64 for sub-millisecond precision).
    ///
    /// Default: no-op. Override for [`PluginMode::Realtime`](crate::PluginMode::Realtime) and
    /// [`PluginMode::Analyzed`](crate::PluginMode::Analyzed) plugins.
    fn process(
        &mut self,
        _samples: &mut [f32],
        _channels: usize,
        _sample_rate: f32,
        _timestamp_secs: f64,
    ) {
    }

    /// Return the current render snapshot as raw bytes.
    ///
    /// Override in plugins that support canvas rendering. The byte format is
    /// plugin-defined; the renderer counterpart must know how to interpret it.
    /// Default: empty slice (plugin does not render).
    fn render_snapshot(&self) -> &[u8] {
        &[]
    }
}

/// Object-safe, `Send`-able runtime interface for audio plugins.
///
/// Used by `PlaybackEngine` to process audio through a plugin chain at runtime.
/// Every concrete type that implements `AudioPlugin + Send` automatically
/// implements this trait via the blanket impl below.
pub trait PluginProcessor: Send {
    /// Process `samples` in-place (interleaved f32, `channels` channels, `sample_rate` Hz).
    /// `timestamp_secs` is the track-relative position of the first sample.
    fn process(
        &mut self,
        samples: &mut [f32],
        channels: usize,
        sample_rate: f32,
        timestamp_secs: f64,
    );

    /// Set a parameter by string ID. Unknown IDs are silently ignored.
    fn set_parameter(&mut self, id: &str, value: f32);

    /// Get a parameter by string ID. Returns `0.0` for unknown IDs.
    fn get_parameter(&self, id: &str) -> f32;

    /// Return the current render snapshot as raw bytes.
    fn render_snapshot(&self) -> &[u8];

    /// Clear internal state (delay lines, reverb tails) on seek. Default: no-op.
    fn reset(&mut self) {}
}

/// Every `T: AudioPlugin + Send` automatically implements `PluginProcessor`.
impl<T: AudioPlugin + Send> PluginProcessor for T {
    fn process(&mut self, samples: &mut [f32], channels: usize, sample_rate: f32, ts: f64) {
        AudioPlugin::process(self, samples, channels, sample_rate, ts);
    }
    fn set_parameter(&mut self, id: &str, value: f32) {
        AudioPlugin::set_parameter(self, id, value);
    }
    fn get_parameter(&self, id: &str) -> f32 {
        AudioPlugin::get_parameter(self, id)
    }
    fn render_snapshot(&self) -> &[u8] {
        AudioPlugin::render_snapshot(self)
    }
}

/// Generate the full C ABI required by the Musicum plugin runtime for a type
/// that implements [`AudioPlugin`].
///
/// # Usage
///
/// ```rust,ignore
/// use audio_plugin_sdk::{AudioPlugin, implement_plugin};
///
/// struct MyPlugin { /* ... */ }
/// impl AudioPlugin for MyPlugin { /* ... */ }
///
/// implement_plugin!(MyPlugin);
/// ```
#[macro_export]
macro_rules! implement_plugin {
    ($ty:ty) => {
        static mut __AP_INSTANCE: Option<$ty> = None;
        static mut __AP_DESCRIPTOR_JSON: Option<String> = None;

        mod __plugin_exports {
            #![allow(static_mut_refs)]
            use super::*;

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_alloc(size: u32) -> u32 {
                let mut buf = Vec::<u8>::with_capacity(size as usize);
                let ptr = buf.as_mut_ptr() as u32;
                std::mem::forget(buf);
                ptr
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_free(ptr: u32, len: u32) {
                unsafe {
                    drop(Vec::from_raw_parts(
                        ptr as *mut u8,
                        len as usize,
                        len as usize,
                    ));
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_new() {
                unsafe {
                    __AP_INSTANCE = Some(<$ty as $crate::AudioPlugin>::new());
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_drop() {
                unsafe {
                    __AP_INSTANCE = None;
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_descriptor_len() -> usize {
                unsafe {
                    if __AP_DESCRIPTOR_JSON.is_none() {
                        __AP_DESCRIPTOR_JSON =
                            Some(<$ty as $crate::AudioPlugin>::descriptor().to_json());
                    }
                    __AP_DESCRIPTOR_JSON.as_ref().unwrap().len()
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_descriptor_ptr() -> u32 {
                unsafe {
                    if __AP_DESCRIPTOR_JSON.is_none() {
                        __AP_DESCRIPTOR_JSON =
                            Some(<$ty as $crate::AudioPlugin>::descriptor().to_json());
                    }
                    __AP_DESCRIPTOR_JSON
                        .as_ref()
                        .map_or(0, |s| s.as_ptr() as u32)
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_set_parameter(id_ptr: u32, id_len: u32, value: f32) {
                unsafe {
                    if let Some(plugin) = __AP_INSTANCE.as_mut() {
                        let id = std::str::from_utf8(std::slice::from_raw_parts(
                            id_ptr as *const u8,
                            id_len as usize,
                        ))
                        .unwrap_or("");
                        plugin.set_parameter(id, value);
                    }
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_get_parameter(id_ptr: u32, id_len: u32) -> f32 {
                unsafe {
                    if let Some(plugin) = __AP_INSTANCE.as_ref() {
                        let id = std::str::from_utf8(std::slice::from_raw_parts(
                            id_ptr as *const u8,
                            id_len as usize,
                        ))
                        .unwrap_or("");
                        return plugin.get_parameter(id);
                    }
                    0.0
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_process(
                buf_ptr: u32,
                buf_len: u32,
                channels: u32,
                sample_rate: f32,
                timestamp_secs: f64,
            ) {
                unsafe {
                    if let Some(plugin) = __AP_INSTANCE.as_mut() {
                        let samples = std::slice::from_raw_parts_mut(
                            buf_ptr as *mut f32,
                            buf_len as usize,
                        );
                        plugin.process(samples, channels as usize, sample_rate, timestamp_secs);
                    }
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __ap_render_snapshot() -> u64 {
                unsafe {
                    if let Some(plugin) = __AP_INSTANCE.as_ref() {
                        let bytes = plugin.render_snapshot();
                        return ((bytes.as_ptr() as u64) << 32) | bytes.len() as u64;
                    }
                    0
                }
            }
        }
    };
}
