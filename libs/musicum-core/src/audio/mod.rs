mod processor_chain;
pub mod player;
pub mod registry;
pub mod source;

use crate::edit::{EditKind, ProcessorEdit};
use structural_processor_sdk::chain::StructuralEdit;

pub use player::PlaybackEngine;
pub use registry::{EditEntry, EditRegistry, EditType, ParamInfo};
pub use source::FileAudioSource;

/// Extract structural edits from a `ProcessorEdit` slice.
/// Plugin edits are silently ignored. Used by `export_service`.
pub fn structural_edits_from(edits: &[ProcessorEdit]) -> Vec<StructuralEdit> {
    edits
        .iter()
        .filter_map(|e| {
            if let EditKind::Structural { processor_id, params } = &e.kind {
                Some(StructuralEdit {
                    processor_id: processor_id.clone(),
                    enabled: e.enabled,
                    params: params.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{EditKind, ProcessorEdit};
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn structural_edits_from_filters_plugins() {
        let edits = vec![
            ProcessorEdit {
                uuid: Uuid::new_v4(), enabled: true,
                kind: EditKind::Structural {
                    processor_id: "trim".to_string(),
                    params: [("start".to_string(), 1.0)].into(),
                },
            },
            ProcessorEdit {
                uuid: Uuid::new_v4(), enabled: true,
                kind: EditKind::Plugin {
                    plugin_id: "gain".to_string(),
                    params: HashMap::new(),
                },
            },
        ];
        let structural = structural_edits_from(&edits);
        assert_eq!(structural.len(), 1);
        assert_eq!(structural[0].processor_id, "trim");
    }

    #[test]
    fn structural_edits_from_preserves_enabled_flag() {
        let edit = ProcessorEdit {
            uuid: Uuid::new_v4(), enabled: false,
            kind: EditKind::Structural {
                processor_id: "cut".to_string(),
                params: HashMap::new(),
            },
        };
        let structural = structural_edits_from(&[edit]);
        assert_eq!(structural[0].enabled, false);
    }
}
