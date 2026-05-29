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
    widgets::Paragraph,
    Frame, Terminal, TerminalOptions, Viewport,
};
use sea_orm::DatabaseConnection;
use std::sync::Arc;

const MAX_QUEUE_VISIBLE: usize = 6;

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
        let (title, items) = build_collection_queue(db, &slug)
            .await
            .map_err(|_| anyhow!("no collection with slug '{slug}'"))?;
        let queue = PlaybackQueue::new(items, Arc::clone(&registry))?;
        if loop_mode { queue.engine().toggle_loop(); }
        return run_player(queue, Some(title), String::new());
    }

    let target = target.ok_or_else(|| anyhow!("provide a target or --collection <slug>"))?;

    // Force modes: only the requested entity type, no collection fallback.
    if force_file || force_clip {
        let (path, edits) = resolve_target(db, &target, force_file, force_clip).await?;
        return play_single_clip(path, edits, loop_mode, registry);
    }

    // Auto-resolve: file slug → clip slug → file path → collection slug.
    if let Ok((path, edits)) = resolve_target(db, &target, false, false).await {
        return play_single_clip(path, edits, loop_mode, registry);
    }

    match build_collection_queue(db, &target).await {
        Ok((title, items)) => {
            let queue = PlaybackQueue::new(items, Arc::clone(&registry))?;
            if loop_mode { queue.engine().toggle_loop(); }
            run_player(queue, Some(title), String::new())
        }
        Err(_) => Err(anyhow!(
            "'{target}' is not a known file, clip, or collection slug, or an existing file path"
        )),
    }
}

async fn build_collection_queue(
    db: &DatabaseConnection,
    slug: &str,
) -> Result<(String, Vec<QueueItem>)> {
    let (col, clips) = collection_service::get_collection_with_clips(db, slug).await?;
    if clips.is_empty() {
        return Err(anyhow!("collection '{slug}' has no clips"));
    }
    let mut items = Vec::with_capacity(clips.len());
    for clip in &clips {
        let file = file_service::get_file_by_id(db, &clip.file_id)
            .await
            .map_err(|_| anyhow!("file not found for clip '{}'", clip.slug))?;
        let edits = deserialize_processor_edits(&clip.processors);
        items.push(QueueItem { title: clip.title.clone(), path: file.path.clone(), edits });
    }
    Ok((col.title, items))
}

fn play_single_clip(
    path: PathBuf,
    edits: Vec<ProcessorEdit>,
    loop_mode: bool,
    registry: Arc<EditRegistry>,
) -> Result<()> {
    let processor_display = format_processor_display(&structural_edits_from(&edits));
    let item = QueueItem {
        title: path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_string(),
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
    _collection_title: Option<String>,
    processor_display: String,
) -> Result<()> {
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());

    let queue_rows = queue.total().min(MAX_QUEUE_VISIBLE) as u16;
    let base_height: u16 = 5  // status, bar, time, hints, separator
        + u16::from(!processor_display.is_empty())
        + queue_rows;

    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions { viewport: Viewport::Inline(base_height) },
    )?;

    let mut queue_scroll: usize = 0;
    let mut queue_exhausted = false;

    loop {
        let ci = queue.current_index();
        if ci < queue_scroll {
            queue_scroll = ci;
        } else if ci >= queue_scroll + MAX_QUEUE_VISIBLE {
            queue_scroll = ci + 1 - MAX_QUEUE_VISIBLE;
        }

        terminal.draw(|f| draw(f, &queue, &processor_display, queue_exhausted, queue_scroll))?;

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
                    (KeyCode::Up, KeyModifiers::NONE) => {
                        if queue.prev() && queue_exhausted {
                            queue_exhausted = false;
                        }
                    }
                    (KeyCode::Down, KeyModifiers::NONE) => {
                        queue.next();
                    }
                    (KeyCode::Right, KeyModifiers::NONE) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek(pos + 3.0);
                    }
                    (KeyCode::Left, KeyModifiers::NONE) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek((pos - 3.0).max(0.0));
                    }
                    (KeyCode::Right, KeyModifiers::SHIFT) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek(pos + 15.0);
                    }
                    (KeyCode::Left, KeyModifiers::SHIFT) => {
                        let pos = queue.engine().position_secs();
                        queue.engine_mut().seek((pos - 15.0).max(0.0));
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
    processor_display: &str,
    queue_exhausted: bool,
    queue_scroll: usize,
) {
    let pos = queue.engine().position_secs();
    let dur = queue.engine().duration_secs();
    let queue_rows = queue.total().min(MAX_QUEUE_VISIBLE);
    let show_processors = !processor_display.is_empty();

    let mut constraints = vec![
        Constraint::Length(1), // status + title
        Constraint::Length(1), // progress bar
        Constraint::Length(1), // time row
        Constraint::Length(1), // hints
    ];
    if show_processors {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1)); // separator
    for _ in 0..queue_rows {
        constraints.push(Constraint::Length(1));
    }

    let areas = Layout::vertical(constraints).split(f.area());
    let mut area_idx = 0usize;

    // status + title
    let (status_icon, status_color) = if queue_exhausted {
        ("■", Color::DarkGray)
    } else if queue.engine().is_paused() {
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

    // progress bar
    let ratio = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    let bar_width = areas[area_idx].width as usize;
    let filled = (ratio * bar_width as f64).floor() as usize;
    let unfilled = bar_width.saturating_sub(filled);
    let bar_str = format!("{}{}", "█".repeat(filled), "░".repeat(unfilled));
    f.render_widget(
        Paragraph::new(bar_str).style(Style::default().fg(Color::White)),
        areas[area_idx],
    );
    area_idx += 1;

    // time row
    let elapsed = fmt_duration(pos);
    let total = fmt_duration(dur);
    let time_width = areas[area_idx].width as usize;
    let gap = time_width.saturating_sub(elapsed.len() + total.len());
    let time_str = format!("{}{}{}", elapsed, " ".repeat(gap), total);
    f.render_widget(
        Paragraph::new(time_str).style(Style::default().fg(Color::DarkGray)),
        areas[area_idx],
    );
    area_idx += 1;

    // hints
    let loop_color = if queue.engine().is_looping() { Color::Cyan } else { Color::DarkGray };
    let hints = Line::from(vec![
        Span::styled("[p] pause  ", Style::default().fg(Color::DarkGray)),
        Span::styled("[l] loop  ", Style::default().fg(loop_color)),
        Span::styled("[↑↓] skip  [←/→] 3s  [S←/S→] 15s  [q] quit", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(hints), areas[area_idx]);
    area_idx += 1;

    // optional processors
    if show_processors {
        f.render_widget(
            Paragraph::new(processor_display).style(Style::default().fg(Color::DarkGray)),
            areas[area_idx],
        );
        area_idx += 1;
    }

    // separator
    let sep = "─".repeat(areas[area_idx].width as usize);
    f.render_widget(
        Paragraph::new(sep).style(Style::default().fg(Color::DarkGray)),
        areas[area_idx],
    );
    area_idx += 1;

    // queue list
    let titles = queue.titles();
    let visible = &titles[queue_scroll..(queue_scroll + queue_rows).min(titles.len())];
    let ci = queue.current_index();
    for (i, title) in visible.iter().enumerate() {
        let abs_idx = queue_scroll + i;
        let line = if abs_idx == ci {
            Line::from(vec![
                Span::raw("▶ "),
                Span::styled(*title, Style::default().add_modifier(Modifier::BOLD)),
            ])
        } else {
            Line::from(vec![Span::raw(format!("  {title}"))])
        };
        f.render_widget(Paragraph::new(line), areas[area_idx]);
        area_idx += 1;
    }
}

fn fmt_duration(secs: f64) -> String {
    let s = secs as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}
