# CLI Unified Output Functions

**Date:** 2026-05-25  
**Scope:** `apps/cli/src/output.rs` + all command files under `apps/cli/src/commands/`

## Goal

Provide two consistent output primitives beyond the existing `print_table`:
- `print_detail` — single-item key-value view with optional named sections
- `print_result` — mutation confirmation line followed by key-value detail

All human-readable CLI output goes through one of `print_table`, `print_detail`, or `print_result`. JSON output continues to use `print_json`.

## DetailItem enum

```rust
pub enum DetailItem<'a> {
    Field(&'a str, String),   // key, value
    Section(&'a str),          // named section header
}
```

`Section` renders as a blank line followed by the label on its own line (no colon, no indentation). Field key alignment is computed globally across all `Field` items in the list — sections do not reset the column width.

## output.rs changes

Replace the current `print_detail(pairs: Vec<(&str, String)>)` with:

```rust
pub fn print_detail(items: &[DetailItem]) {
    let key_w = items.iter()
        .filter_map(|i| if let DetailItem::Field(k, _) = i { Some(k.len()) } else { None })
        .max()
        .unwrap_or(0);
    for item in items {
        match item {
            DetailItem::Field(key, val) => println!("{key:>key_w$}: {val}"),
            DetailItem::Section(title)  => println!("\n{title}"),
        }
    }
}

pub fn print_result(action: &str, items: &[DetailItem]) {
    println!("{action}");
    if !items.is_empty() {
        print_detail(items);
    }
}
```

`print_result` with an empty slice prints only the action line — used for destructive operations like `remove`.

## Caller changes per command

### presets show
Use `print_detail` with a flat list of fields. The processors sub-table stays as a separate `println!("\nprocessors:")` + `print_table` call (mixed table + detail, no change needed there).

### presets create
Replace the ad-hoc `println!` block with:
```
print_result("Created preset", &[
    Field("slug",        slug),
    Field("title",       title),
    Field("description", description or "-"),
    Field("processors",  "(none — use 'presets add-processor <slug> <type>' to add)"),
])
```

### presets remove
```
print_result(&format!("Removed preset '{slug}'"), &[])
```

### presets add-processor
Replace bare `println!("{instance_id}")` with:
```
print_result("Added processor", &[
    Field("id",    instance_id),
    Field("preset", preset_slug),
    Field("type",   processor_type),
])
```

### presets remove-processor
```
print_result(&format!("Removed processor '{instance_uuid}'"), &[
    Field("preset", preset_slug),
])
```

### clips show
Replace the flat list + empty-string separator hack with a single `print_detail` call using `Section("file")` to divide clip fields from file fields. Keys like `"file:duration"` become just `"duration"` under the section.

### clips create
```
print_result("Created clip", &[
    Field("slug", clip.slug),
    Field("file", file_slug),
])
```

### files show
Build a `Vec<DetailItem>` for the file fields, then conditionally push `Section("metadata")` + metadata fields if `meta` is `Some`. Pass the vec to `print_detail`. The clips sub-table stays as a separate `print_table` call.

### collections show
Update call to new `print_detail` signature (no section needed — flat list of three fields).

## Rendering example

```
presets show my-preset

    slug: my-preset
   title: My Preset
     desc: For film scoring sessions

processors
  UUID  KIND        PROC   ENABLED  PARAMS
  ──────────────────────────────────────────
  abc…  structural  crop   true     start=0
```

```
presets create --title "Film"

Created preset
    slug: film
   title: Film
  processors: (none — use 'presets add-processor film <type>' to add)
```

```
clips show my-clip

    slug: my-clip
   title: My Clip
  cached: false

file
    slug: my-file
    path: /music/file.flac
duration: 3.141s
```

## What does NOT change

- `print_table` signature and behaviour
- `print_json`
- The processors sub-table inside `presets show` (stays as `println! + print_table`)
- The clips sub-table inside `files show` (stays as `print_table`)
- JSON output paths (`--json` flag handling in every command)
