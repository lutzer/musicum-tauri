# CLI Unified Table Component Implementation Plan

**Goal:** Replace `print_table` (2-col) and `print_table_3col` (3-col) with a single
`print_table(headers: &[&str], rows: Vec<Vec<String>>)` that auto-sizes columns and
spans the full terminal width.

**Architecture:** A private `format_table(headers, rows, term_w) -> String` does all
the logic and is unit-testable. The public `print_table` detects the terminal width
via `crossterm` and calls `format_table`. All six call sites across five command files
are mechanically migrated.

**Tech Stack:** Rust, `crossterm 0.28` (already in `apps/cli/Cargo.toml`)

**Spec:** `docs/plans/specs/2026-05-25-cli-unified-table.md`

---

## File map

| File | Action |
|------|--------|
| `apps/cli/src/output.rs` | Rewrite `print_table`, add `format_table`, delete `print_table_3col` |
| `apps/cli/src/commands/files.rs` | Migrate 2 call sites |
| `apps/cli/src/commands/clips.rs` | Migrate 2 call sites |
| `apps/cli/src/commands/presets.rs` | Migrate 1 call site + replace hand-rolled processor table |
| `apps/cli/src/commands/collections.rs` | Migrate 1 call site |
| `apps/cli/src/commands/processors.rs` | Migrate 1 call site, update import |

---

### Task 1: Add `format_table` stub and write failing tests

**Files:**
- Modify: `apps/cli/src/output.rs`

Add the private stub and a `#[cfg(test)]` block at the bottom of `output.rs`.
The tests will fail to compile until Task 2 fills in the body.

```rust
// Add after print_detail — before the closing brace of the file

fn format_table(headers: &[&str], rows: &[Vec<String>], term_w: usize) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::format_table;

    #[test]
    fn separator_spans_full_width() {
        let out = format_table(
            &["SLUG", "TITLE"],
            &[vec!["my-preset".into(), "My Preset".into()]],
            40,
        );
        let sep = out.lines().nth(1).unwrap();
        assert_eq!(sep.chars().count(), 40);
    }

    #[test]
    fn two_col_basic_layout() {
        let out = format_table(
            &["SLUG", "TITLE"],
            &[
                vec!["my-preset".into(), "My Preset".into()],
                vec!["another".into(), "Another One".into()],
            ],
            60,
        );
        let lines: Vec<&str> = out.lines().collect();
        // header + separator + 2 rows
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("SLUG"));
        assert!(lines[2].starts_with("my-preset"));
        assert!(lines[3].starts_with("another  "));
    }

    #[test]
    fn three_col_layout() {
        let out = format_table(
            &["ID", "NAME", "PARAMS"],
            &[vec!["crop".into(), "Crop".into(), "start=0 (time)".into()]],
            60,
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        // header has all three column names
        assert!(lines[0].contains("ID"));
        assert!(lines[0].contains("NAME"));
        assert!(lines[0].contains("PARAMS"));
    }

    #[test]
    fn last_col_truncates_with_ellipsis() {
        let long_val = "a".repeat(100);
        let out = format_table(
            &["ID", "VALUE"],
            &[vec!["x".into(), long_val]],
            20,
        );
        // "x" (1) + "  " (2) = 3 fixed, last col gets 17, truncated to 16 + …
        let row = out.lines().nth(2).unwrap();
        assert!(row.contains('…'));
        // display width should not exceed term_w
        let last_cell_start = row.find("  ").unwrap() + 2;
        let last_cell = &row[last_cell_start..];
        assert!(last_cell.chars().count() <= 17);
    }

    #[test]
    fn last_col_fits_without_truncation() {
        let out = format_table(
            &["ID", "VALUE"],
            &[vec!["x".into(), "short".into()]],
            40,
        );
        let row = out.lines().nth(2).unwrap();
        assert!(!row.contains('…'));
        assert!(row.contains("short"));
    }

    #[test]
    fn col_width_driven_by_widest_row() {
        let out = format_table(
            &["ID", "VALUE"],
            &[
                vec!["short".into(), "v1".into()],
                vec!["much-longer-slug".into(), "v2".into()],
            ],
            60,
        );
        let lines: Vec<&str> = out.lines().collect();
        // Both rows should align — first col padded to len("much-longer-slug")=16
        assert!(lines[2].starts_with("short            "));
    }
}
```

Run to confirm compile error (not a test failure — `todo!()` panics at runtime, not
compile time, so this will actually compile and panic):
```
cargo test -p musicum-cli 2>&1 | head -30
```
Expected: tests compile but `separator_spans_full_width` panics at `todo!()`.

---

### Task 2: Implement `format_table` and update `print_table`

**Files:**
- Modify: `apps/cli/src/output.rs`

Replace the `todo!()` stub with the full implementation, and rewrite `print_table`
to use it. Keep `print_table_3col` for now (deleted in Task 8 after its call site is
migrated).

Replace the `format_table` function body:

```rust
fn format_table(headers: &[&str], rows: &[Vec<String>], term_w: usize) -> String {
    let ncols = headers.len();
    if ncols == 0 {
        return String::new();
    }

    // Width of each non-last column = max(header len, widest cell in that column)
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for i in 0..ncols.saturating_sub(1) {
            let cell_len = row.get(i).map(|s| s.len()).unwrap_or(0);
            widths[i] = widths[i].max(cell_len);
        }
    }

    // Last column fills remaining terminal space (floor: header length)
    let fixed_w: usize = widths[..ncols - 1].iter().sum::<usize>() + (ncols - 1) * 2;
    let last_w = term_w.saturating_sub(fixed_w).max(headers[ncols - 1].len());
    widths[ncols - 1] = last_w;

    let mut out = String::new();

    // Header row
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        if i < ncols - 1 {
            out.push_str(&format!("{:<width$}", h, width = widths[i]));
        } else {
            out.push_str(h);
        }
    }
    out.push('\n');

    // Separator — full terminal width
    for _ in 0..term_w {
        out.push('─');
    }
    out.push('\n');

    // Data rows
    for row in rows {
        for i in 0..ncols {
            if i > 0 {
                out.push_str("  ");
            }
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            if i < ncols - 1 {
                out.push_str(&format!("{:<width$}", cell, width = widths[i]));
            } else {
                // Last column: truncate with … if over budget
                let char_count = cell.chars().count();
                if char_count <= widths[i] {
                    out.push_str(cell);
                } else {
                    let truncated: String = cell.chars().take(widths[i].saturating_sub(1)).collect();
                    out.push_str(&truncated);
                    out.push('…');
                }
            }
        }
        out.push('\n');
    }

    out
}
```

Replace the existing `print_table` function (the old 2-col version):

```rust
pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    let term_w = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    print!("{}", format_table(headers, &rows, term_w));
}
```

Add the `crossterm` import at the top of `output.rs` (if not already present):
```rust
use crossterm::terminal;
```

Then update `print_table` to use it:
```rust
pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>) {
    let term_w = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    print!("{}", format_table(headers, &rows, term_w));
}
```

---

### Task 3: Run tests — verify all pass

```
cargo test -p musicum-cli
```

Expected output (all tests in `output::tests` pass):
```
running 6 tests
test output::tests::separator_spans_full_width ... ok
test output::tests::two_col_basic_layout ... ok
test output::tests::three_col_layout ... ok
test output::tests::last_col_truncates_with_ellipsis ... ok
test output::tests::last_col_fits_without_truncation ... ok
test output::tests::col_width_driven_by_widest_row ... ok

test result: ok. 6 passed; 0 failed
```

If any test fails, fix `format_table` before proceeding.

---

### Task 4: Migrate `files.rs` — 2 call sites

**Files:**
- Modify: `apps/cli/src/commands/files.rs`

**Site 1 — `FilesCommand::List`** (around line 38):
```rust
// BEFORE
print_table(
    ("SLUG", "NAME  [DURATION  RATE  CH]"),
    files
        .iter()
        .map(|f| {
            (
                f.slug.clone(),
                format!(
                    "{}  [{:.1}s  {}Hz  {}ch]",
                    f.name, f.duration, f.sample_rate, f.channels
                ),
            )
        })
        .collect(),
);

// AFTER
print_table(
    &["SLUG", "NAME  [DURATION  RATE  CH]"],
    files
        .iter()
        .map(|f| vec![
            f.slug.clone(),
            format!("{}  [{:.1}s  {}Hz  {}ch]", f.name, f.duration, f.sample_rate, f.channels),
        ])
        .collect(),
);
```

**Site 2 — `FilesCommand::Show` clips sub-table** (around line 93):
```rust
// BEFORE
print_table(
    ("SLUG", "TITLE  [CACHED]"),
    clips
        .iter()
        .map(|c| {
            (c.slug.clone(), format!("{}  [{}]", c.title, c.cached))
        })
        .collect(),
);

// AFTER
print_table(
    &["SLUG", "TITLE  [CACHED]"],
    clips
        .iter()
        .map(|c| vec![c.slug.clone(), format!("{}  [{}]", c.title, c.cached)])
        .collect(),
);
```

Verify it compiles:
```
cargo build -p musicum-cli 2>&1 | grep -E "^error"
```
Expected: no output (no errors).

---

### Task 5: Migrate `clips.rs` — 2 call sites

**Files:**
- Modify: `apps/cli/src/commands/clips.rs`

**Site 1 — with `file_slug`** (around line 48):
```rust
// BEFORE
print_table(
    ("SLUG", "TITLE  [CACHED]"),
    clips.iter().map(|c| (c.slug.clone(), format!("{}  [{}]", c.title, c.cached))).collect(),
);

// AFTER
print_table(
    &["SLUG", "TITLE  [CACHED]"],
    clips.iter().map(|c| vec![c.slug.clone(), format!("{}  [{}]", c.title, c.cached)]).collect(),
);
```

**Site 2 — all clips** (around line 64):
```rust
// BEFORE
print_table(
    ("SLUG", "FILE  TITLE  [CACHED]"),
    clips.iter().map(|c| {
        let file_slug = file_slugs.get(&c.file_id).map(|s| s.as_str()).unwrap_or("?");
        (c.slug.clone(), format!("{}  {}  [{}]", file_slug, c.title, c.cached))
    }).collect(),
);

// AFTER
print_table(
    &["SLUG", "FILE  TITLE  [CACHED]"],
    clips.iter().map(|c| {
        let file_slug = file_slugs.get(&c.file_id).map(|s| s.as_str()).unwrap_or("?");
        vec![c.slug.clone(), format!("{}  {}  [{}]", file_slug, c.title, c.cached)]
    }).collect(),
);
```

Verify:
```
cargo build -p musicum-cli 2>&1 | grep -E "^error"
```

---

### Task 6: Migrate `presets.rs` — list call site + hand-rolled processor table

**Files:**
- Modify: `apps/cli/src/commands/presets.rs`

**Site 1 — `PresetsCommand::List`** (around line 58):
```rust
// BEFORE
print_table(
    ("SLUG", "TITLE"),
    presets.iter().map(|p| (p.slug.clone(), p.title.clone())).collect(),
);

// AFTER
print_table(
    &["SLUG", "TITLE"],
    presets.iter().map(|p| vec![p.slug.clone(), p.title.clone()]).collect(),
);
```

**Site 2 — `PresetsCommand::Show` processor sub-table** (around line 80–99):

Remove the entire hand-rolled block:
```rust
// REMOVE THIS ENTIRE BLOCK:
let uuid_w = 36;
let kind_w = 12;
let proc_w = 6;
println!("  {:<uuid_w$}  {:<kind_w$}  {:<proc_w$}  ENABLED  PARAMS",
    "UUID", "KIND", "PROC");
println!("  {}", "─".repeat(uuid_w + kind_w + proc_w + 30));
for entry in &processors {
    let (id, kind, proc_id, enabled, params) = match entry {
        ProcessorEntry::Structural { id, enabled, processor } => (
            id.as_str(), "structural", processor.id.as_str(), *enabled,
            format_params(&processor.params),
        ),
        ProcessorEntry::AudioPlugin { id, enabled, processor } => (
            id.as_str(), "audio-plugin", processor.id.as_str(), *enabled,
            format_params(&processor.params),
        ),
    };
    println!("  {id:<uuid_w$}  {kind:<kind_w$}  {proc_id:<proc_w$}  {enabled:<7}  {params}");
}
```

Replace with:
```rust
print_table(
    &["UUID", "KIND", "PROC", "ENABLED", "PARAMS"],
    processors.iter().map(|entry| {
        let (id, kind, proc_id, enabled, params) = match entry {
            ProcessorEntry::Structural { id, enabled, processor } => (
                id.as_str(), "structural", processor.id.as_str(), *enabled,
                format_params(&processor.params),
            ),
            ProcessorEntry::AudioPlugin { id, enabled, processor } => (
                id.as_str(), "audio-plugin", processor.id.as_str(), *enabled,
                format_params(&processor.params),
            ),
        };
        vec![id.to_string(), kind.to_string(), proc_id.to_string(),
             enabled.to_string(), params]
    }).collect(),
);
```

Also update the surrounding context — the `if processors.is_empty()` branch stays, but
the `else` block now just does `println!("\nprocessors:"); print_table(...)`:

```rust
if processors.is_empty() {
    println!("\nprocessors: (none)");
} else {
    println!("\nprocessors:");
    print_table(
        &["UUID", "KIND", "PROC", "ENABLED", "PARAMS"],
        processors.iter().map(|entry| { /* ... as above ... */ }).collect(),
    );
}
```

Verify:
```
cargo build -p musicum-cli 2>&1 | grep -E "^error"
```

---

### Task 7: Migrate `collections.rs` — 1 call site

**Files:**
- Modify: `apps/cli/src/commands/collections.rs`

```rust
// BEFORE
print_table(
    ("SLUG", "TITLE"),
    cols.iter().map(|c| (c.slug.clone(), c.title.clone())).collect(),
);

// AFTER
print_table(
    &["SLUG", "TITLE"],
    cols.iter().map(|c| vec![c.slug.clone(), c.title.clone()]).collect(),
);
```

Verify:
```
cargo build -p musicum-cli 2>&1 | grep -E "^error"
```

---

### Task 8: Migrate `processors.rs` and delete `print_table_3col`

**Files:**
- Modify: `apps/cli/src/commands/processors.rs`
- Modify: `apps/cli/src/output.rs`

**Step 1 — update `processors.rs`:**

Update the import at the top:
```rust
// BEFORE
use crate::output::{print_json, print_table_3col};

// AFTER
use crate::output::{print_json, print_table};
```

Replace the call site (around line 33):
```rust
// BEFORE
let rows: Vec<(String, String, String)> = entries
    .iter()
    .map(|e| {
        let d = (e.descriptor)();
        let params = d.parameters.iter()
            .map(|p| match p {
                ParameterDescriptor::Time { id, default, .. } =>
                    format!("{id}={default} (time)"),
                ParameterDescriptor::Int { id, default, .. } =>
                    format!("{id}={default} (int)"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        (d.id.to_string(), d.name.to_string(), params)
    })
    .collect();
print_table_3col(("ID", "NAME", "PARAMETERS"), rows);

// AFTER
print_table(
    &["ID", "NAME", "PARAMETERS"],
    entries
        .iter()
        .map(|e| {
            let d = (e.descriptor)();
            let params = d.parameters.iter()
                .map(|p| match p {
                    ParameterDescriptor::Time { id, default, .. } =>
                        format!("{id}={default} (time)"),
                    ParameterDescriptor::Int { id, default, .. } =>
                        format!("{id}={default} (int)"),
                })
                .collect::<Vec<_>>()
                .join(", ");
            vec![d.id.to_string(), d.name.to_string(), params]
        })
        .collect(),
);
```

**Step 2 — delete `print_table_3col` from `output.rs`:**

Remove the entire function (lines ~22–33 in the original file):
```rust
// DELETE THIS ENTIRE FUNCTION:
pub fn print_table_3col(
    headers: (&str, &str, &str),
    rows: Vec<(String, String, String)>,
) {
    let c1 = rows.iter().map(|(a, _, _)| a.len()).max().unwrap_or(0).max(headers.0.len());
    let c2 = rows.iter().map(|(_, b, _)| b.len()).max().unwrap_or(0).max(headers.1.len());
    println!("{:<c1$}  {:<c2$}  {}", headers.0, headers.1, headers.2);
    println!("{}", "─".repeat(c1 + c2 + 4 + headers.2.len().min(60)));
    for (a, b, c) in rows {
        println!("{a:<c1$}  {b:<c2$}  {c}");
    }
}
```

Verify full build:
```
cargo build -p musicum-cli 2>&1 | grep -E "^error"
```
Expected: no output.

---

### Task 9: Final lint and test run

```
cargo clippy -p musicum-cli -- -D warnings
cargo test -p musicum-cli
```

Expected:
- `clippy`: no warnings
- `test`: all 6 tests in `output::tests` pass, no regressions

If clippy flags unused imports (e.g. `ParameterDescriptor` variants no longer needed
in any file), fix them before marking done.
