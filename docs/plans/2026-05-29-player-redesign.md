# Player UI Redesign Implementation Plan

**Goal:** Redesign the CLI player to show a white custom progress bar with time below it, and a vertical scrollable queue list with the current track highlighted.

**Architecture:** All changes are isolated to `apps/cli/src/commands/play.rs`. The `run_player` function gains a `queue_scroll` variable; the `draw` function is refactored into focused sub-draws. The `Gauge` widget is replaced by a custom `Paragraph`-based progress bar.

**Tech Stack:** Rust, ratatui 0.29, crossterm

---

### Task 1: Update viewport height calculation

**Files:**
- Modify: `apps/cli/src/commands/play.rs:163-181`

Remove the old height formula that accounts for the collection header. Replace with:

```rust
const MAX_QUEUE_VISIBLE: usize = 6;

fn run_player(
    mut queue: PlaybackQueue,
    _collection_title: Option<String>,   // kept in signature, no longer used for header
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
    // ... rest of loop unchanged for now
```

- Remove the `in_collection` variable and `collection_title` header logic from the height formula.
- Add `queue_scroll: usize = 0` variable to the outer loop scope.
- Add `const MAX_QUEUE_VISIBLE: usize = 6;` at module level (above `run_player`).

Run `cargo clippy -p musicum-cli` and fix any warnings before proceeding.

---

### Task 2: Update key bindings

**Files:**
- Modify: `apps/cli/src/commands/play.rs:194-221`

Replace the key match block inside the event loop:

```rust
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
```

- Remove the old `(KeyCode::Right, KeyModifiers::SHIFT) => { queue.next(); }` and `(KeyCode::Left, KeyModifiers::SHIFT) => { ... queue.prev() ... }` arms.
- The `queue_exhausted` reset on `queue.prev()` now belongs to the `Up` arm.

Run `cargo clippy -p musicum-cli`.

---

### Task 3: Pass `queue_scroll` into `draw` and update its signature

**Files:**
- Modify: `apps/cli/src/commands/play.rs:185,228`

Update the `draw` call site in the loop:

```rust
// update scroll before drawing
let ci = queue.current_index();
if ci < queue_scroll {
    queue_scroll = ci;
} else if ci >= queue_scroll + MAX_QUEUE_VISIBLE {
    queue_scroll = ci + 1 - MAX_QUEUE_VISIBLE;
}

terminal.draw(|f| draw(f, &queue, &processor_display, queue_exhausted, queue_scroll))?;
```

Update the `draw` function signature:

```rust
fn draw(
    f: &mut Frame,
    queue: &PlaybackQueue,
    processor_display: &str,
    queue_exhausted: bool,
    queue_scroll: usize,
) {
```

- Remove the `collection_title: &Option<String>` parameter from both call site and definition.
- Remove all uses of `in_collection` and `collection_title` inside `draw`.

Run `cargo clippy -p musicum-cli`.

---

### Task 4: Rebuild the layout constraints inside `draw`

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — inside `draw`, the constraints block

Replace the constraints-building block with:

```rust
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
```

- No collection-header row added.

Run `cargo clippy -p musicum-cli`.

---

### Task 5: Render status + title row

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — inside `draw`, after `area_idx = 0`

Replace the old collection-header block and title block with:

```rust
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
```

Run `cargo clippy -p musicum-cli`.

---

### Task 6: Replace `Gauge` with custom progress bar paragraph

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — inside `draw`, progress bar section

Remove the `Gauge` import from the `use ratatui::widgets` block (keep `Paragraph`).

Replace the gauge render block with:

```rust
let pos = queue.engine().position_secs();
let dur = queue.engine().duration_secs();
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
```

Run `cargo clippy -p musicum-cli`.

---

### Task 7: Render time row

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — inside `draw`, after progress bar

Add the time row immediately after the progress bar block:

```rust
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
```

Run `cargo clippy -p musicum-cli`.

---

### Task 8: Update hints row

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — inside `draw`, hints section

Replace the hints block (remove the separate collection hints row):

```rust
let loop_color = if queue.engine().is_looping() { Color::Cyan } else { Color::DarkGray };
let hints = Line::from(vec![
    Span::styled("[p] pause  ", Style::default().fg(Color::DarkGray)),
    Span::styled("[l] loop  ", Style::default().fg(loop_color)),
    Span::styled("[↑↓] skip  [←/→] 3s  [S←/S→] 15s  [q] quit", Style::default().fg(Color::DarkGray)),
]);
f.render_widget(Paragraph::new(hints), areas[area_idx]);
area_idx += 1;
```

Run `cargo clippy -p musicum-cli`.

---

### Task 9: Render optional processor display

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — inside `draw`, processor section

Keep the processor display block unchanged except for `area_idx` ordering:

```rust
if show_processors {
    f.render_widget(
        Paragraph::new(processor_display).style(Style::default().fg(Color::DarkGray)),
        areas[area_idx],
    );
    area_idx += 1;
}
```

Run `cargo clippy -p musicum-cli`.

---

### Task 10: Render separator and queue list

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — inside `draw`, at end

After the optional processor row, add:

```rust
// separator
let sep = "─".repeat(areas[area_idx].width as usize);
f.render_widget(
    Paragraph::new(sep).style(Style::default().fg(Color::DarkGray)),
    areas[area_idx],
);
area_idx += 1;

// queue rows
let titles: Vec<&str> = queue.titles();
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
```

**Note:** This requires `PlaybackQueue` to expose a `titles()` method returning `Vec<&str>`. Check `libs/musicum-core/src/` for the queue struct and add it if missing (see Task 11).

Run `cargo clippy -p musicum-cli`.

---

### Task 11: Add `titles()` method to `PlaybackQueue` if missing

**Files:**
- Modify: `libs/musicum-core/src/` — wherever `PlaybackQueue` is defined

Find `PlaybackQueue`:

```bash
grep -rn "struct PlaybackQueue" libs/musicum-core/src/
```

If there is no `titles()` method, add:

```rust
pub fn titles(&self) -> Vec<&str> {
    self.items.iter().map(|item| item.title.as_str()).collect()
}
```

Also verify `current_title()`, `current_index()`, and `total()` already exist (they are used by the current code). If `total()` returns `usize`, no changes needed.

Run `cargo test -p musicum-core` and `cargo clippy --all`.

---

### Task 12: Clean up unused imports

**Files:**
- Modify: `apps/cli/src/commands/play.rs` — top-level imports

Remove `Gauge` from the `ratatui::widgets` import since it is no longer used:

```rust
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, Terminal, TerminalOptions, Viewport,
};
```

Run `cargo clippy --all` and confirm zero warnings.

---

### Task 13: End-to-end verification

```bash
# Build
cargo build -p musicum-cli

# Single track (confirm: 1-item queue highlighted, white bar, time below)
cargo run -p musicum-cli -- play <any-file-slug>

# Collection (confirm: queue scrolls, ↑/↓ skips, ←/→ seeks 3s, Shift+←/→ seeks 15s)
cargo run -p musicum-cli -- play --collection <any-collection-slug>

# Collection with >6 tracks (confirm: max 6 rows visible, scrolls with track changes)
cargo run -p musicum-cli -- play --collection <large-collection-slug>

# Final lint
cargo clippy --all
```
