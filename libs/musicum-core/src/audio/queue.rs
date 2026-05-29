use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::audio::player::PlaybackEngine;
use crate::audio::registry::EditRegistry;
use crate::edit::ProcessorEdit;

pub struct QueueItem {
    pub title: String,
    pub path:  String,
    pub edits: Vec<ProcessorEdit>,
}

pub struct PlaybackQueue {
    items:         Vec<QueueItem>,
    current_index: usize,
    engine:        PlaybackEngine,
    registry:      Arc<EditRegistry>,
}

impl PlaybackQueue {
    pub fn new(items: Vec<QueueItem>, registry: Arc<EditRegistry>) -> Result<Self> {
        if items.is_empty() {
            return Err(anyhow!("PlaybackQueue requires at least one item"));
        }
        let engine = PlaybackEngine::new(
            Path::new(&items[0].path),
            &items[0].edits,
            &registry,
        )?;
        engine.play();
        Ok(Self { items, current_index: 0, engine, registry })
    }

    pub fn engine(&self)         -> &PlaybackEngine     { &self.engine }
    pub fn engine_mut(&mut self) -> &mut PlaybackEngine { &mut self.engine }
    pub fn current_index(&self)  -> usize               { self.current_index }
    pub fn total(&self)          -> usize                { self.items.len() }
    pub fn current_title(&self)  -> &str                { &self.items[self.current_index].title }

    pub fn next(&mut self) -> bool {
        if self.current_index + 1 >= self.items.len() {
            return false;
        }
        self.current_index += 1;
        self.replace_engine();
        true
    }

    /// If current position > 3 s: seek to 0.
    /// Otherwise go to previous clip if any; if already at 0 with low position: no-op (false).
    pub fn prev(&mut self) -> bool {
        if self.engine.position_secs() > 3.0 {
            self.engine.seek(0.0);
            return true;
        }
        if self.current_index == 0 {
            return false;
        }
        self.current_index -= 1;
        self.replace_engine();
        true
    }

    /// Call once per TUI tick. Returns `true` if the engine was advanced to the next clip.
    /// Returns `false` when the last clip has finished (queue exhausted).
    pub fn advance_if_finished(&mut self) -> bool {
        if !self.engine.is_finished() {
            return false;
        }
        if self.current_index + 1 >= self.items.len() {
            return false;
        }
        self.current_index += 1;
        self.replace_engine();
        true
    }

    fn replace_engine(&mut self) {
        let item = &self.items[self.current_index];
        if let Ok(eng) = PlaybackEngine::new(Path::new(&item.path), &item.edits, &self.registry) {
            eng.play();
            self.engine = eng;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::registry::EditRegistry;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    fn temp_wav(frames: usize, sample_rate: u32) -> NamedTempFile {
        let tmp = NamedTempFile::new().unwrap();
        let spec = WavSpec { channels: 1, sample_rate, bits_per_sample: 32,
                             sample_format: SampleFormat::Float };
        let mut w = WavWriter::create(tmp.path(), spec).unwrap();
        for i in 0..frames { w.write_sample(i as f32 / frames as f32).unwrap(); }
        w.finalize().unwrap();
        tmp
    }

    #[test]
    fn new_single_item_sets_index_zero() {
        let tmp = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![QueueItem {
            title: "track".to_string(),
            path: tmp.path().to_str().unwrap().to_string(),
            edits: vec![],
        }];
        let queue = PlaybackQueue::new(items, registry).unwrap();
        assert_eq!(queue.current_index(), 0);
        assert_eq!(queue.total(), 1);
        assert_eq!(queue.current_title(), "track");
    }

    #[test]
    fn new_empty_items_returns_error() {
        let registry = Arc::new(EditRegistry::default());
        let result = PlaybackQueue::new(vec![], registry);
        assert!(result.is_err());
    }

    #[test]
    fn next_advances_index() {
        let tmp1 = temp_wav(4410, 44_100);
        let tmp2 = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![
            QueueItem { title: "a".to_string(), path: tmp1.path().to_str().unwrap().to_string(), edits: vec![] },
            QueueItem { title: "b".to_string(), path: tmp2.path().to_str().unwrap().to_string(), edits: vec![] },
        ];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        let moved = queue.next();
        assert!(moved);
        assert_eq!(queue.current_index(), 1);
        assert_eq!(queue.current_title(), "b");
    }

    #[test]
    fn next_at_last_returns_false() {
        let tmp = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![QueueItem { title: "only".to_string(),
                                     path: tmp.path().to_str().unwrap().to_string(),
                                     edits: vec![] }];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        assert!(!queue.next());
        assert_eq!(queue.current_index(), 0);
    }

    #[test]
    fn prev_at_start_with_low_position_returns_false() {
        let tmp = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![QueueItem { title: "only".to_string(),
                                     path: tmp.path().to_str().unwrap().to_string(),
                                     edits: vec![] }];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        // position is 0, index is 0: no-op
        assert!(!queue.prev());
    }

    #[test]
    fn prev_at_index_1_moves_back() {
        let tmp1 = temp_wav(4410, 44_100);
        let tmp2 = temp_wav(4410, 44_100);
        let registry = Arc::new(EditRegistry::default());
        let items = vec![
            QueueItem { title: "a".to_string(), path: tmp1.path().to_str().unwrap().to_string(), edits: vec![] },
            QueueItem { title: "b".to_string(), path: tmp2.path().to_str().unwrap().to_string(), edits: vec![] },
        ];
        let mut queue = PlaybackQueue::new(items, registry).unwrap();
        queue.next();
        let moved = queue.prev();
        assert!(moved);
        assert_eq!(queue.current_index(), 0);
    }
}
