pub mod chain;
pub mod processor;
pub mod source;

pub use chain::{StructuralEdit, build_chain, chain_output_duration,
                map_time_forward, map_time_back, descriptors_json, validate_edit};
pub use processor::{
    ParameterDescriptor, Params, ProcessorDescriptor,
    StreamingProcessorInstance, StructuralProcessor,
};
pub use source::{AudioSource, VecAudioSource, secs_to_samples};

use std::collections::HashMap;

/// Vtable entry for one structural processor. Static fn pointers allow
/// time-mapping and duration queries without constructing an instance.
pub struct ProcessorEntry {
    pub descriptor:       fn() -> &'static ProcessorDescriptor,
    pub validate:         fn(&Params) -> bool,
    pub create:           fn(Params) -> Box<dyn StreamingProcessorInstance>,
    pub output_duration:  fn(f64, &Params) -> f64,
    pub map_time_forward: fn(f64, f64, &Params) -> f64,
    pub map_time_back:    fn(f64, f64, &Params) -> f64,
}

impl ProcessorEntry {
    pub fn of<P: StructuralProcessor>() -> Self {
        Self {
            descriptor:       P::descriptor,
            validate:         P::validate,
            create:           P::create,
            output_duration:  P::output_duration,
            map_time_forward: P::map_time_forward,
            map_time_back:    P::map_time_back,
        }
    }
}

/// Processor registry: maps processor ID → entry. Built once at startup.
pub type Registry = HashMap<String, ProcessorEntry>;
