pub mod chain;
pub mod processor;

pub use chain::Edit;
pub use processor::{ParameterDescriptor, Params, ProcessorDescriptor, StructuralProcessor};

/// Vtable entry for one processor. Holds plain function pointers — no heap, no trait objects.
pub struct StructuralProcessorEntry {
    pub descriptor:       fn() -> &'static ProcessorDescriptor,
    pub validate:         fn(&Params) -> bool,
    pub apply:            fn(&[f32], u32, u16, &Params) -> Vec<f32>,
    pub output_duration:  fn(f64, &Params) -> f64,
    pub map_time_forward: fn(f64, f64, &Params) -> f64,
    pub map_time_back:    fn(f64, f64, &Params) -> f64,
}

impl StructuralProcessorEntry {
    pub fn of<P: StructuralProcessor>() -> Self {
        Self {
            descriptor:       P::descriptor,
            validate:         P::validate,
            apply:            P::apply,
            output_duration:  P::output_duration,
            map_time_forward: P::map_time_forward,
            map_time_back:    P::map_time_back,
        }
    }
}

/// Generate all WASM C-ABI exports for a structural processor chain.
///
/// Pass a comma-separated list of processor types. A static registry is
/// built once from these types; all `__sp_*` exports delegate to the
/// SDK chain functions with that registry.
///
/// # Usage
///
/// ```rust,ignore
/// structural_processor_sdk::implement_sp_chain!(
///     TrimProcessor, CutProcessor, SliceProcessor, CropProcessor,
/// );
/// ```
#[macro_export]
macro_rules! implement_sp_chain {
    ($($proc:ty),+ $(,)?) => {
        static __SP_REGISTRY_CELL: std::sync::OnceLock<Vec<$crate::StructuralProcessorEntry>> =
            std::sync::OnceLock::new();

        fn __sp_registry() -> &'static [$crate::StructuralProcessorEntry] {
            __SP_REGISTRY_CELL.get_or_init(|| {
                vec![$($crate::StructuralProcessorEntry::of::<$proc>()),+]
            })
        }

        static mut __SP_RESULT: Vec<f32> = Vec::new();
        static mut __SP_DESCRIPTORS: Option<String> = None;

        mod __sp_exports {
            #![allow(static_mut_refs)]
            use super::*;

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_alloc(size: u32) -> u32 {
                let mut buf = Vec::<u8>::with_capacity(size as usize);
                let ptr = buf.as_mut_ptr() as u32;
                std::mem::forget(buf);
                ptr
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_free(ptr: u32, len: u32) {
                unsafe {
                    drop(Vec::from_raw_parts(
                        ptr as *mut u8,
                        len as usize,
                        len as usize,
                    ));
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_apply_chain(
                samples_ptr: u32, samples_len: u32,
                sample_rate: u32, channels: u32,
                edits_ptr: u32, edits_len: u32,
            ) {
                unsafe {
                    let samples = std::slice::from_raw_parts(
                        samples_ptr as *const f32, samples_len as usize,
                    );
                    let edits_json = std::str::from_utf8(std::slice::from_raw_parts(
                        edits_ptr as *const u8, edits_len as usize,
                    ))
                    .unwrap_or("[]");
                    let edits: Vec<$crate::chain::Edit> =
                        serde_json::from_str(edits_json).unwrap_or_default();
                    __SP_RESULT = $crate::chain::apply_chain(
                        __sp_registry(), samples, sample_rate, channels as u16, &edits,
                    );
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_result_ptr() -> u32 {
                unsafe { __SP_RESULT.as_ptr() as u32 }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_result_len() -> u32 {
                unsafe { __SP_RESULT.len() as u32 }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_descriptors_init() {
                unsafe {
                    if __SP_DESCRIPTORS.is_none() {
                        __SP_DESCRIPTORS =
                            Some($crate::chain::descriptors_json(__sp_registry()));
                    }
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_descriptors_ptr() -> u32 {
                unsafe {
                    __SP_DESCRIPTORS.as_ref().map_or(0, |s| s.as_ptr() as u32)
                }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_descriptors_len() -> u32 {
                unsafe { __SP_DESCRIPTORS.as_ref().map_or(0, |s| s.len() as u32) }
            }

            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_validate_edit(
                type_ptr: u32, type_len: u32,
                params_ptr: u32, params_len: u32,
            ) -> u32 {
                unsafe {
                    let edit_type = std::str::from_utf8(std::slice::from_raw_parts(
                        type_ptr as *const u8, type_len as usize,
                    ))
                    .unwrap_or("");
                    let params_json = std::str::from_utf8(std::slice::from_raw_parts(
                        params_ptr as *const u8, params_len as usize,
                    ))
                    .unwrap_or("{}");
                    let params: $crate::Params =
                        serde_json::from_str(params_json).unwrap_or_default();
                    if $crate::chain::validate_edit(__sp_registry(), edit_type, &params) {
                        1
                    } else {
                        0
                    }
                }
            }

            /// Map a source time forward through the edit chain.
            /// `duration` is the total raw audio duration in seconds.
            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_map_time_forward(
                edits_ptr: u32, edits_len: u32,
                t: f64,
                duration: f64,
            ) -> f64 {
                unsafe {
                    let edits_json = std::str::from_utf8(std::slice::from_raw_parts(
                        edits_ptr as *const u8, edits_len as usize,
                    ))
                    .unwrap_or("[]");
                    let edits: Vec<$crate::chain::Edit> =
                        serde_json::from_str(edits_json).unwrap_or_default();
                    $crate::chain::map_time_forward(__sp_registry(), &edits, t, duration)
                }
            }

            /// Map a processed time backward through the edit chain.
            /// `duration` is the total raw audio duration in seconds.
            #[cfg_attr(not(test), no_mangle)]
            pub extern "C" fn __sp_map_time_back(
                edits_ptr: u32, edits_len: u32,
                t: f64,
                duration: f64,
            ) -> f64 {
                unsafe {
                    let edits_json = std::str::from_utf8(std::slice::from_raw_parts(
                        edits_ptr as *const u8, edits_len as usize,
                    ))
                    .unwrap_or("[]");
                    let edits: Vec<$crate::chain::Edit> =
                        serde_json::from_str(edits_json).unwrap_or_default();
                    $crate::chain::map_time_back(__sp_registry(), &edits, t, duration)
                }
            }
        }
    };
}
