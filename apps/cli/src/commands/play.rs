use std::{io, path::PathBuf, time::Duration};

use anyhow::{anyhow, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use musicum_core::{
    audio::PlaybackEngine,
    services::{clip_service, file_service},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame, Terminal,
};
use sea_orm::DatabaseConnection;

pub async fn run(db: &DatabaseConnection, target: String, force_file: bool, force_clip: bool) -> Result<()> {
    let path = resolve_path(db, &target, force_file, force_clip).await?;
    let engine = PlaybackEngine::new(&path, &[])?;
    engine.play();
    run_player(engine)
}

async fn resolve_path(
    db: &DatabaseConnection,
    target: &str,
    force_file: bool,
    force_clip: bool,
) -> Result<PathBuf> {
    if force_file {
        let file = file_service::get_file_by_slug(db, target).await
            .map_err(|_| anyhow!("no file with slug '{target}'"))?;
        return Ok(PathBuf::from(file.path));
    }

    if force_clip {
        let clip = clip_service::get_clip_by_slug(db, target).await
            .map_err(|_| anyhow!("no clip with slug '{target}'"))?;
        let file = file_service::get_file_by_id(db, &clip.file_id).await
            .map_err(|_| anyhow!("parent file for clip '{target}' not found"))?;
        return Ok(PathBuf::from(file.path));
    }

    if let Ok(file) = file_service::get_file_by_slug(db, target).await {
        return Ok(PathBuf::from(file.path));
    }
    if let Ok(clip) = clip_service::get_clip_by_slug(db, target).await {
        if let Ok(file) = file_service::get_file_by_id(db, &clip.file_id).await {
            return Ok(PathBuf::from(file.path));
        }
    }
    let path = PathBuf::from(target);
    if path.exists() {
        return Ok(path);
    }
    Err(anyhow!("'{target}' is not a known slug or an existing file path"))
}

fn run_player(engine: PlaybackEngine) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| draw(f, &engine))?;

        if engine.is_finished() {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('p'), _) => engine.toggle_pause(),
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
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn draw(f: &mut Frame, engine: &PlaybackEngine) {
    let pos = engine.position_secs();
    let dur = engine.duration_secs();
    let paused = engine.is_paused();

    let [_, center, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(7),
        Constraint::Fill(1),
    ])
    .areas(f.area());

    let title = format!(" {} ", engine.title());
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(center);
    f.render_widget(block, center);

    let [gauge_area, status_area, _, hints_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let ratio = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(ratio)
        .label(format!("{} / {}", fmt_duration(pos), fmt_duration(dur)));
    f.render_widget(gauge, gauge_area);

    let (status_text, status_color) = if paused {
        ("⏸  Paused", Color::Yellow)
    } else {
        ("▶  Playing", Color::Green)
    };
    f.render_widget(
        Paragraph::new(status_text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(status_color)),
        status_area,
    );

    f.render_widget(
        Paragraph::new("[p] pause  [←/→] seek 5s  [q] quit")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::DarkGray)),
        hints_area,
    );
}

fn fmt_duration(secs: f64) -> String {
    let s = secs as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}
