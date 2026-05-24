use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};

use anyhow::{anyhow, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::Print,
    terminal,
};
use musicum_core::{
    audio::PlaybackEngine,
    services::{clip_service, file_service},
};
use sea_orm::DatabaseConnection;

pub async fn run(db: &DatabaseConnection, target: String, force_file: bool, force_clip: bool) -> Result<()> {
    let path = resolve_path(db, &target, force_file, force_clip).await?;
    let engine = PlaybackEngine::new(&path)?;
    engine.play();
    run_tui(engine)
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

    // Auto-resolve: try file slug, then clip slug, then literal path
    if let Ok(file) = file_service::get_file_by_slug(db, target).await {
        return Ok(PathBuf::from(file.path));
    }
    if let Ok(clip) = clip_service::get_clip_by_slug(db, target).await {
        if let Ok(file) = file_service::get_file_by_id(db, &clip.file_id).await {
            return Ok(PathBuf::from(file.path));
        }
    }
    // Fall back to literal file path
    let path = PathBuf::from(target);
    if path.exists() {
        return Ok(path);
    }
    Err(anyhow!("'{}' is not a known slug or an existing file path", target))
}

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        // Move cursor below the player UI and show it again
        let mut stdout = io::stdout();
        let _ = execute!(stdout, cursor::Show);
    }
}

fn run_tui(engine: PlaybackEngine) -> Result<()> {
    let mut stdout = io::stdout();
    terminal::enable_raw_mode()?;
    let _guard = RawModeGuard;

    execute!(stdout, cursor::Hide)?;

    // Print initial blank lines that the renderer will overwrite
    for _ in 0..5 {
        writeln!(stdout, "\r")?;
    }

    let mut first = true;
    loop {
        render(&engine, &mut stdout, first)?;
        first = false;

        if engine.is_finished() {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(KeyEvent {
                    code: KeyCode::Char('p'),
                    ..
                }) => engine.toggle_pause(),

                Event::Key(KeyEvent {
                    code: KeyCode::Right,
                    ..
                }) => engine.seek(engine.position_secs() + 5.0),

                Event::Key(KeyEvent {
                    code: KeyCode::Left,
                    ..
                }) => engine.seek((engine.position_secs() - 5.0).max(0.0)),

                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Esc, ..
                })
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                }) => break,

                _ => {}
            }
        }
    }

    // Move below UI before exiting
    queue!(stdout, cursor::MoveDown(1))?;
    writeln!(stdout, "\r")?;
    stdout.flush()?;
    Ok(())
}

fn render(engine: &PlaybackEngine, stdout: &mut impl Write, first: bool) -> Result<()> {
    let pos = engine.position_secs();
    let dur = engine.duration_secs();
    let paused = engine.is_paused();

    let (term_width, _) = terminal::size().unwrap_or((80, 24));
    // bar_width = total width minus "[" + "]" + " mm:ss / mm:ss" (14 chars) + 2 padding
    let bar_width = (term_width as usize).saturating_sub(20).max(5);

    let filled = if dur > 0.0 {
        ((pos / dur) * bar_width as f64).round() as usize
    } else {
        0
    }
    .min(bar_width);
    let empty = bar_width - filled;

    let dur_str = fmt_duration(dur);
    let pos_str = fmt_duration(pos);
    let status = if paused { "⏸  Paused " } else { "▶  Playing" };

    let title_line = format!(" {}  [{}]", engine.title(), dur_str);
    let sep: String = "─".repeat(term_width as usize);
    let bar_line = format!(
        " [{}{}] {} / {}",
        "█".repeat(filled),
        "░".repeat(empty),
        pos_str,
        dur_str
    );
    let status_line = format!(" {status}");
    let hint_line = " [p] pause  [←/→] seek 5s  [q] quit";

    if !first {
        queue!(stdout, cursor::MoveUp(5))?;
    }

    for line in &[&title_line, &sep, &bar_line, &status_line, hint_line] {
        queue!(
            stdout,
            terminal::Clear(terminal::ClearType::CurrentLine),
            Print(format!("{line}\r\n")),
        )?;
    }

    stdout.flush()?;
    Ok(())
}

fn fmt_duration(secs: f64) -> String {
    let s = secs as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}
