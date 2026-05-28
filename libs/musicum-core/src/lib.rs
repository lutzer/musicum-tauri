pub mod audio;
pub mod config;
pub mod db;
pub mod edit;
pub mod error;
pub mod services;
pub mod sidecar;

pub use audio::{structural_edits_from, EditEntry, EditRegistry, EditType, ParamInfo, PlaybackEngine};
pub use structural_processor_sdk::chain::StructuralEdit;
pub use edit::{deserialize_processor_edits, EditKind, ProcessorEdit};
pub use error::ServiceError;
