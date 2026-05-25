# CLI Unified Table Component

**Date:** 2026-05-25
**Status:** approved

## Goal

Replace the two ad-hoc table functions (`print_table`, `print_table_3col`) and one
hand-rolled table in `presets show` with a single `print_table` function that:

- Accepts any number of columns
- Fills the full terminal width with its separator line
- Truncates the last column with `…` instead of overflowing

## Function signature

```rust
// output.rs
pub fn print_table(headers: &[&str], rows: Vec<Vec<String>>)
```

`headers` length determines the column count. Every `Vec<String>` in `rows` must have
the same length as `headers`; extra cells are ignored, missing cells are treated as
empty strings.

## Rendering rules

### Terminal width

Detected once per call via `crossterm::terminal::size()`. Falls back to `80` if the
call fails (e.g. stdout is not a tty).

### Column widths

1. For each column `i` in `0..ncols-1` (all but the last):
   `width[i] = max(header[i].len(), max over rows of row[i].len())`
2. Fixed prefix width: `sum(width[0..ncols-1]) + (ncols - 1) * 2` (two spaces between columns)
3. Last column width: `terminal_width.saturating_sub(fixed_prefix_width)`, with a floor
   of `header[ncols-1].len()` so the header is never cut.

### Separator

A single line of `─` characters repeated to `terminal_width`, printed after the header
row. No partial lines.

### Row rendering

- Columns `0..ncols-1` are left-aligned and padded to their fixed width.
- The last column is printed as-is if `<= last_col_width`, or truncated to
  `last_col_width - 1` characters and suffixed with `…` otherwise.
- Two spaces (`  `) separate every adjacent column pair.

### Output format (example, 80-col terminal)

```
SLUG              NAME          PARAMETERS
────────────────────────────────────────────────────────────────────────────────
crop              Crop          start=0 (time), end=0 (time)
trim              Trim          threshold=-40 (int)
```

## Deleted functions

| Old function      | Replacement             |
|-------------------|-------------------------|
| `print_table`     | new `print_table`       |
| `print_table_3col`| new `print_table`       |

`print_detail` is unaffected.

## Call-site migration

Each call site converts its row tuples to `Vec<String>` and passes `&[&str]` headers.

### files list
```rust
// before
print_table(
    ("SLUG", "NAME  [DURATION  RATE  CH]"),
    files.iter().map(|f| (f.slug.clone(), format!(...))).collect(),
);

// after
print_table(
    &["SLUG", "NAME  [DURATION  RATE  CH]"],
    files.iter().map(|f| vec![f.slug.clone(), format!(...)]).collect(),
);
```

### clips list (both variants — with and without file_slug)
Tuple `(slug, info)` → `vec![slug, info]`. Headers unchanged.

### presets list
Tuple `(slug, title)` → `vec![slug, title]`. Headers unchanged.

### collections list
Tuple `(slug, title)` → `vec![slug, title]`. Headers unchanged.

### processors list (was print_table_3col)
```rust
// before
print_table_3col(("ID", "NAME", "PARAMETERS"), rows);
// rows: Vec<(String, String, String)>

// after
print_table(
    &["ID", "NAME", "PARAMETERS"],
    rows.into_iter().map(|(a, b, c)| vec![a, b, c]).collect(),
);
```

### presets show — processor sub-table (hand-rolled → unified)
The inline loop that prints processor rows with hardcoded `uuid_w`, `kind_w`, `proc_w`
is replaced by:

```rust
print_table(
    &["UUID", "KIND", "PROC", "ENABLED", "PARAMS"],
    processors.iter().map(|entry| {
        let (id, kind, proc_id, enabled, params) = /* same extraction as today */;
        vec![id.to_string(), kind.to_string(), proc_id.to_string(),
             enabled.to_string(), params]
    }).collect(),
);
```

## Dependencies

`crossterm` is already declared in `apps/cli/Cargo.toml`. No new dependencies needed.

## Out of scope

- Column alignment options (right-align for numbers) — YAGNI
- Colour/ANSI styling
- Pagination
