pub mod player;
pub mod source;

use crate::sidecar::ProcessorEntry;
use structural_processor_sdk::chain::StructuralEdit;

pub use player::PlaybackEngine;
pub use source::FileAudioSource;

/// Convert sidecar [`ProcessorEntry`] items into [`StructuralEdit`]s for the audio chain.
/// `AudioPlugin` entries are filtered out — only `Structural` entries are used.
pub fn sidecar_entries_to_edits(entries: &[ProcessorEntry]) -> Vec<StructuralEdit> {
    entries
        .iter()
        .filter_map(|e| {
            if let ProcessorEntry::Structural { enabled, processor, .. } = e {
                let params = processor
                    .params
                    .as_object()
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_f64().map(|f| (k.clone(), f)))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(StructuralEdit { processor_id: processor.id.clone(), enabled: *enabled, params })
            } else {
                None
            }
        })
        .collect()
}
