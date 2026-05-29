use std::{io, path::PathBuf, time::Duration};

use anyhow::{anyhow, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use musicum_core::{
    audio::structural_edits_from,
    deserialize_processor_edits,
    edit::ProcessorEdit,
    EditRegistry, PlaybackQueue, QueueItem, StructuralEdit,
    services::{clip_service, collection_service, file_service},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Gauge, Paragraph},
    Frame, Terminal, TerminalOptions, Viewport,
};
use sea_orm::DatabaseConnection;
use std::sync::Arc;

fn format_processor_display(edits: &[StructuralEdit]) -> String {
    edits
        .iter()
        .filter(|e| e.enabled)
        .map(|e| {
            let mut params: Vec<String> = e
                .params
                .iter()
                .map(|(k, v)| format!("{k}={v:.2}s"))
                .collect();
            params.sort();
            format!("{} {}", e.processor_id, params.join(" "))
        })
        .collect::<Vec<_>>()
        .join("  ")
}

pub async fn run(
    db: &DatabaseConnection,
    target: Option<String>,
    collection: Option<String>,
    force_file: bool,
    force_clip: bool,
    loop_mode: bool,
) -> Result<()> {
    let registry = Arc::new(EditRegistry::default());

    if let Some(slug) = collection {
        let (col, clips) = collection_service::get_collection_with_clips(db, &slug)
            .await
            .map_err(|_| anyhow!("no collection with slug '{slug}'"))?;

        if clips.is_empty() {
            return Err(anyhow!("collection '{slug}' has no clips"));
        }

        let mut items: Vec<QueueItem> = Vec::with_capacity(clips.len());
        for clip in &clips {
            let file = file_service::get_file_by_id(db, &clip.file_id)
                .await
                .map_err(|_| anyhow!("file not found for clip '{}'", clip.slug))?;
            let edits = deserialize_processor_edits(&clip.processors);
            items.push(QueueItem {
                title: clip.title.clone(),
                path:  file.path.clone(),
                edits,
            });
        }

        let queue = PlaybackQueue::new(items, Arc::clone(&registry))?;
        if loop_mode { queue.engine().toggle_loop(); }
        return run_player(queue, Some(col.title), String::new());
    }

    let target = target.ok_or_else(|| anyhow!("provide a target or --collection <slug>"))?;
    let (path, edits) = resolve_target(db, &target, force_file, force_clip).await?;
    let structural = structural_edits_from(&edits);
    let processor_display = format_processor_display(&structural);

    let item = QueueItem {
        title: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string(),
        path:  path.to_string_lossy().to_string(),
        edits,
    };
    let queue = PlaybackQueue::new(vec![item], Arc::clone(&registry))?;
    if loop_mode { queue.engine().toggle_loop(); }
    run_player(queue, None, processor_display)
}

async fn resolve_target(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<(PathBuf, Vec<ProcessorEdit>)> {
    if force_file {
        let file = file_service::get_file_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no file with slug '{target}'"))?;
        return Ok((PathBuf::from(file.path), vec![]));
    }

    if force_clip {
        let clip = clip_service::get_clip_by_slug(db, target)
            .await
            .map_err(|_| anyhow!("no clip with slug '{target}'"))?;
        let file = file_service::get_file_by_id(db, &clip.file_id)
            .await
            .map_err(|_| anyhow!("parent file for clip '{target}' not found"))?;
        let edits = deserialize_processor_edits(&clip.processors);
        return Ok((PathBuf::from(file.path), edits));
    }

    if let Ok(file) = file_service::get_file_by_slug(db, target).await {
        return Ok((PathBuf::from(file.path), vec![]));
    }
    if let Ok(clip) = clip_service::get_clip_by_slug(db, target).await {
        if let Ok(file) = file_service::get_file_by_id(db, &clip.file_id).await {
            let edits = deserialize_processor_edits(&clip.processors);
            return Ok((PathBuf::from(file.path), edits));
        }
    }
    let path = PathBuf::from(target);
    if path.exists() {
        return Ok((path, vec![]));
    }
    Err(anyhow!("'{target}' is not a known slug or an existing file path"))
}

fn run_player(
    mut queue: PlaybackQueue,
    collection_title: Option<String>,
    processor_display: String,
) -> Result<()> {
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());

    let in_collection = collection_title.is_some();
    let base_height = 3u16
        + u16::from(!processor_display.is_empty())
        + u16::from(in_collection)
        + u16::from(in_collection);

    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions { viewport: Viewport::Inline(base_height) },
    )?;

    let mut queue_exhausted = false;

    loop {
        terminal.draw(|f| draw(f, &queue, &collection_title, &processor_display, queue_exhausted))?;

        if !queue_exhausted && queue.advance_if_finished() {
            // engine replaced; loop continues
        }
        if !queue_exhausted && queue.engine().is_finished() && queue.current_index() + 1 >= queue.total() {
            queue_exhausted = true;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    _ if queue_exhausted => {}
                    (KeyCode::Char('p'), _) => queue.engine_mut().toggle_pause(),
                    (KeyCode::Char('l'), _) => queue.engine_mut().toggle_loop(),
                    (KeyCode::Right, KeyModifiers::NONE) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek(pos + 5.0);
                    }
                    (KeyCode::Left, KeyModifiers::NONE) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek((pos - 5.0).max(0.0));
                    }
                    (KeyCode::Right, KeyModifiers::SHIFT) => { queue.next(); }
                    (KeyCode::Left,  KeyModifiers::SHIFT) => {
                        if queue.prev() && queue_exhausted {
                            queue_exhausted = false;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn draw(
    f: &mut Frame,
    queue: &PlaybackQueue,
    collection_title: &Option<String>,
    processor_display: &str,
    queue_exhausted: bool,
) {
    let engine = queue.engine();
    let pos = engine.position_secs();
    let dur = engine.duration_secs();
    let paused = engine.is_paused();
    let in_collection = collection_title.is_some();

    let show_processors = !processor_display.is_empty();
    let mut constraints = Vec::new();
    if in_collection  { constraints.push(Constraint::Length(1)); }
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(1));
    if show_processors { constraints.push(Constraint::Length(1)); }
    constraints.push(Constraint::Length(1));
    if in_collection  { constraints.push(Constraint::Length(1)); }

    let areas = Layout::vertical(constraints).split(f.area());
    let mut area_idx = 0usize;

    if let Some(title) = collection_title {
        let badge = format!("{}/{}", queue.current_index() + 1, queue.total());
        let header = Line::from(vec![
            Span::styled(title.as_str(), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(badge, Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(header), areas[area_idx]);
        area_idx += 1;
    }

    let (status_icon, status_color) = if queue_exhausted {
        ("■", Color::DarkGray)
    } else if paused {
        ("⏸", Color::Yellow)
    } else {
        ("▶", Color::Green)
    };
    let title_line = Line::from(vec![
        Span::styled(status_icon, Style::default().fg(status_color)),
        Span::raw("  "),
        Span::styled(queue.current_title(), Style::default().add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(title_line), areas[area_idx]);
    area_idx += 1;

    let ratio = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(ratio)
            .label(format!("{} / {}", fmt_duration(pos), fmt_duration(dur))),
        areas[area_idx],
    );
    area_idx += 1;

    if show_processors {
        f.render_widget(
            Paragraph::new(processor_display).style(Style::default().fg(Color::DarkGray)),
            areas[area_idx],
        );
        area_idx += 1;
    }

    let loop_color = if engine.is_looping() { Color::Cyan } else { Color::DarkGray };
    let base_hints = Line::from(vec![
        Span::styled("[p] pause  ", Style::default().fg(Color::DarkGray)),
        Span::styled("[l] loop  ", Style::default().fg(loop_color)),
        Span::styled("[←/→] seek 5s  [q] quit", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(base_hints), areas[area_idx]);
    area_idx += 1;

    if in_collection {
        let skip_hints = Line::from(vec![
            Span::styled("[S←/S→] prev/next clip", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(skip_hints), areas[area_idx]);
    }
}

fn fmt_duration(secs: f64) -> String {
    let s = secs as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}
