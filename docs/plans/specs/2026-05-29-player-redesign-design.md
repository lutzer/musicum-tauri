# Player UI Redesign

## Context

The current player uses ratatui's `Gauge` widget with a colored progress bar and embeds the time label inside it. Queue context is only partially visible (a `1/5` counter), key bindings for skip and seek conflict, and there is no way to see or navigate the full queue visually. This redesign gives the queue a first-class vertical list, tightens the progress display, and rationalises the key bindings.

## Layout

```
▶  Late Summer Rain
████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░
00:42                                    03:14
[p] pause  [l] loop  [↑↓] skip  [←/→] 3s  [q] quit
────────────────────────────────────────────
  Cold Snap
▶ Late Summer Rain
  Moonrise
  Drift
```

Rows, in order:

1. **Status + title** — play/pause/stop icon followed by the current track title.
2. **Progress bar** — full-width custom paragraph: `n` `█` chars (filled) + remaining `░` chars (unfilled), all white foreground, no background color.
3. **Time row** — elapsed time left-aligned, total duration right-aligned, on one line below the bar.
4. **Hints** — key binding reminder, updated for new bindings.
5. *(optional)* **Processor display** — shown only when processors are active; dark-gray.
6. **Separator** — a horizontal rule `─` spanning the full width.
7. **Queue list** — vertical list of track titles, max 6 visible rows, scrolls to keep the current item in view. Current item prefixed with `▶` and rendered bold; other items indented with two spaces.

Total viewport height: `5 + (1 if processors) + min(queue_len, 6)` rows.

The old collection title/counter header row is removed; the queue list makes it redundant.

## Key Bindings

| Key | Action |
|-----|--------|
| `↑` | Previous track |
| `↓` | Next track |
| `←` | Seek −3 s |
| `→` | Seek +3 s |
| `Shift+←` | Seek −15 s |
| `Shift+→` | Seek +15 s |
| `p` | Toggle pause |
| `l` | Toggle loop |
| `q` / `Esc` / `Ctrl+C` | Quit |

## Progress Bar Rendering

Replace the `Gauge` widget with a `Paragraph`. At draw time:

1. Compute `filled = floor(ratio * area.width)` characters of `█`.
2. Remaining characters: `░`.
3. Render as a single `Span` with `Style::default().fg(Color::White)`.

## Queue Scroll Logic

Maintain a `queue_scroll: usize` offset in the draw loop (or passed into `draw`). On each draw:

- If `current_index < queue_scroll` → set `queue_scroll = current_index`.
- If `current_index >= queue_scroll + MAX_QUEUE_VISIBLE` → set `queue_scroll = current_index - MAX_QUEUE_VISIBLE + 1`.

`MAX_QUEUE_VISIBLE = 6`.

## Files Changed

- `apps/cli/src/commands/play.rs` — all changes are contained here: `run_player`, `draw`, key handler, and a new `draw_progress_bar` helper.

## Verification

```
# Single track
cargo run -p musicum-cli -- play <slug>
# Verify: queue shows 1 item highlighted, progress bar is white █/░, time below bar

# Collection
cargo run -p musicum-cli -- play --collection <slug>
# Verify: queue scrolls, ↑/↓ skips tracks, ←/→ seeks 3s, Shift+←/→ seeks 15s

# Large collection (>6 tracks)
# Verify: queue caps at 6 rows and scrolls to keep current track visible
```
