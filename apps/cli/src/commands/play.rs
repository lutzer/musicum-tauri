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
    EditRegistry, PlaybackEngine,
    services::{clip_service, file_service},
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
use structural_processor_sdk::chain::StructuralEdit;

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

pub async fn run(db: &DatabaseConnection, target: String, force_file: bool, force_clip: bool, loop_mode: bool) -> Result<()> {
    let (path, edits) = resolve_target(db, &target, force_file, force_clip).await?;
    // Show structural edits in the display (plugins are silent in the TUI for now)
    let structural = structural_edits_from(&edits);
    let processor_display = format_processor_display(&structural);
    let registry = EditRegistry::default();
    let engine = PlaybackEngine::new(&path, &edits, &registry)?;
    if loop_mode { engine.toggle_loop(); }
    engine.play();
    run_player(engine, processor_display)
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

fn run_player(engine: PlaybackEngine, processor_display: String) -> Result<()> {
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(io::stdout());
    let height = if processor_display.is_empty() { 3 } else { 4 };
    let mut terminal = Terminal::with_options(
        backend,
        TerminalOptions { viewport: Viewport::Inline(height) },
    )?;

    loop {
        terminal.draw(|f| draw(f, &engine, &processor_display))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('p'), _) => engine.toggle_pause(),
                    (KeyCode::Char('l'), _) => engine.toggle_loop(),
                    (KeyCode::Right, _) => engine.seek(engine.position_secs() + 5.0),
                    (KeyCode::Left, _) => engine.seek((engine.position_secs() - 5.0).max(0.0)),
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn draw(f: &mut Frame, engine: &PlaybackEngine, processor_display: &str) {
    let pos = engine.position_secs();
    let dur = engine.duration_secs();
    let paused = engine.is_paused();

    let show_processors = !processor_display.is_empty();
    let constraints = if show_processors {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    };
    let areas = Layout::vertical(constraints).split(f.area());

    let (status_text, status_color) = if paused {
        ("⏸  Paused", Color::Yellow)
    } else {
        ("▶  Playing", Color::Green)
    };
    let header = Line::from(vec![
        Span::styled(engine.title(), Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]);
    f.render_widget(Paragraph::new(header), areas[0]);

    let ratio = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    f.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(ratio)
            .label(format!("{} / {}", fmt_duration(pos), fmt_duration(dur))),
        areas[1],
    );

    let loop_color = if engine.is_looping() { Color::Cyan } else { Color::DarkGray };
    let hints = Line::from(vec![
        Span::styled("[p] pause  ", Style::default().fg(Color::DarkGray)),
        Span::styled("[l] loop  ", Style::default().fg(loop_color)),
        Span::styled("[←/→] seek 5s  [q] quit", Style::default().fg(Color::DarkGray)),
    ]);

    if show_processors {
        f.render_widget(
            Paragraph::new(processor_display)
                .style(Style::default().fg(Color::DarkGray)),
            areas[2],
        );
        f.render_widget(Paragraph::new(hints), areas[3]);
    } else {
        f.render_widget(Paragraph::new(hints), areas[2]);
    }
}

fn fmt_duration(secs: f64) -> String {
    let s = secs as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}
