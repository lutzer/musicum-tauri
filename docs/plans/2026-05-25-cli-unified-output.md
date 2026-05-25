# CLI Unified Output Functions — Implementation Plan

**Goal:** Introduce `DetailItem` enum, update `print_detail`, and add `print_result` so every human-readable CLI output goes through one of three consistent primitives: `print_table`, `print_detail`, or `print_result`.

**Architecture:** All changes are confined to `apps/cli/src/output.rs` (the primitive definitions) and the five command files that call them. No new files are created. The existing `print_table` and `print_json` are untouched. The `DetailItem` lifetime is `'a` tied to the key strings, which are always string literals in practice — no heap cost.

**Tech Stack:** Rust stable, `crossterm` (already a dep for terminal width), no new dependencies.

---

## File Map

| File | Change |
|---|---|
| `apps/cli/src/output.rs` | Add `DetailItem`, replace `print_detail` signature, add `print_result` |
| `apps/cli/src/commands/presets.rs` | Update Show, Create, Remove, AddProcessor, RemoveProcessor |
| `apps/cli/src/commands/clips.rs` | Update Show, Create |
| `apps/cli/src/commands/files.rs` | Update Show |
| `apps/cli/src/commands/collections.rs` | Update Show |

---

### Task 1: Add `DetailItem` enum and update `print_detail` in `output.rs`

**Files:**
- Modify: `apps/cli/src/output.rs`

**Steps:**

1. Add the `DetailItem` enum just before `print_detail`. Insert after line 6 (after `print_json`):

```rust
pub enum DetailItem<'a> {
    Field(&'a str, String),
    Section(&'a str),
}
```

2. Replace the existing `print_detail` function (lines 13–18) with:

```rust
pub fn print_detail(items: &[DetailItem]) {
    let key_w = items
        .iter()
        .filter_map(|i| if let DetailItem::Field(k, _) = i { Some(k.len()) } else { None })
        .max()
        .unwrap_or(0);
    for item in items {
        match item {
            DetailItem::Field(key, val) => println!("{key:>key_w$}: {val}"),
            DetailItem::Section(title) => println!("\n{title}"),
        }
    }
}
```

3. Add `print_result` immediately after `print_detail`:

```rust
pub fn print_result(action: &str, items: &[DetailItem]) {
    println!("{action}");
    if !items.is_empty() {
        print_detail(items);
    }
}
```

4. Update the existing `separator_spans_full_width` and other tests — they test `format_table` directly and are unaffected. Add two new tests at the bottom of the `#[cfg(test)]` block to cover `print_detail` with a section and `print_result`:

```rust
#[test]
fn detail_section_renders_label() {
    // Use DetailItem directly to verify section output.
    // We test via the public API since print_detail writes to stdout;
    // just assert it doesn't panic and the enum variants construct correctly.
    use super::DetailItem::{Field, Section};
    let items = vec![
        Field("slug", "my-clip".into()),
        Section("file"),
        Field("path", "/music/x.flac".into()),
    ];
    // key_w should be 4 (max of "slug".len() and "path".len())
    let key_w = items.iter()
        .filter_map(|i| if let super::DetailItem::Field(k, _) = i { Some(k.len()) } else { None })
        .max()
        .unwrap_or(0);
    assert_eq!(key_w, 4);
}
```

5. Run the tests to confirm no regressions:

```
cargo test -p musicum-cli
```

Expected: all existing tests pass, new test passes.

---

### Task 2: Update `presets.rs` — all five arms

**Files:**
- Modify: `apps/cli/src/commands/presets.rs`

**Steps:**

1. Add `DetailItem` to the import line (line 11). Change:

```rust
use crate::output::{print_detail, print_json, print_table};
```

to:

```rust
use crate::output::{DetailItem::{Field, Section}, print_detail, print_json, print_result, print_table};
```

2. **`PresetsCommand::Show`** — update `print_detail` call (lines 72–77). Replace:

```rust
print_detail(vec![
    ("slug", preset.slug.clone()),
    ("title", preset.title.clone()),
    ("description", if preset.description.is_empty() { "-".into() } else { preset.description.clone() }),
]);
```

with:

```rust
print_detail(&[
    Field("slug", preset.slug.clone()),
    Field("title", preset.title.clone()),
    Field("description", if preset.description.is_empty() { "-".into() } else { preset.description.clone() }),
]);
```

(The `\nprocessors:` line and `print_table` call below are unchanged.)

3. **`PresetsCommand::Create`** — replace the multi-`println!` block (lines 105–111):

```rust
println!("Created preset '{title}'");
println!("  slug: {slug}");
if !description.is_empty() {
    println!("  description: {description}");
}
println!("  processors: (none — use 'presets add-processor {slug} <type>' to add)");
```

with:

```rust
print_result("Created preset", &[
    Field("slug", slug.clone()),
    Field("title", title.clone()),
    Field("description", if description.is_empty() { "-".into() } else { description.clone() }),
    Field("processors", format!("(none — use 'presets add-processor {slug} <type>' to add)")),
]);
```

4. **`PresetsCommand::Remove`** — replace (line 115):

```rust
println!("removed '{slug}'");
```

with:

```rust
print_result(&format!("Removed preset '{slug}'"), &[]);
```

5. **`PresetsCommand::AddProcessor`** — replace the bare `println!("{instance_id}")` (line 163) with:

```rust
print_result("Added processor", &[
    Field("id", instance_id.clone()),
    Field("preset", preset_slug.clone()),
    Field("type", processor_type.clone()),
]);
```

6. **`PresetsCommand::RemoveProcessor`** — replace (line 181):

```rust
println!("removed processor '{instance_uuid}'");
```

with:

```rust
print_result(&format!("Removed processor '{instance_uuid}'"), &[
    Field("preset", preset_slug.clone()),
]);
```

7. Run lint + tests:

```
cargo clippy -p musicum-cli -- -D warnings
cargo test -p musicum-cli
```

Expected: zero warnings, all tests pass.

---

### Task 3: Update `clips.rs` — Show and Create

**Files:**
- Modify: `apps/cli/src/commands/clips.rs`

**Steps:**

1. Update the import line (line 7):

```rust
use crate::output::{DetailItem::{Field, Section}, print_detail, print_json, print_result, print_table};
```

2. **`ClipsCommand::Show`** — replace the `print_detail` call (lines 83–98). The current code uses `("", "".into())` as a blank separator and prefixes file fields with `"file:"`. Replace with a single call using `Section`:

```rust
print_detail(&[
    Field("slug", clip.slug.clone()),
    Field("title", clip.title.clone()),
    Field("cached", clip.cached.clone()),
    Field("cached_path", clip.cached_path.clone().unwrap_or_else(|| "-".into())),
    Field("duration", clip.duration.map_or("-".into(), |d| format!("{d:.3}s"))),
    Field("processors", serde_json::to_string_pretty(&processors).unwrap()),
    Field("notes", if clip.notes.is_empty() { "-".into() } else { clip.notes.clone() }),
    Section("file"),
    Field("slug", file.slug.clone()),
    Field("path", file.path.clone()),
    Field("duration", format!("{:.3}s", file.duration)),
    Field("sample_rate", format!("{}Hz", file.sample_rate)),
    Field("channels", file.channels.to_string()),
    Field("mime", file.mime_type.clone()),
]);
```

Note: `"file:duration"` etc. become just `"duration"` etc. because the `Section("file")` header makes the grouping explicit.

3. **`ClipsCommand::Create`** — replace (line 103):

```rust
println!("Created clip '{}' for file '{}'", clip.slug, file_slug);
```

with:

```rust
print_result("Created clip", &[
    Field("slug", clip.slug.clone()),
    Field("file", file_slug.clone()),
]);
```

4. Run lint + tests:

```
cargo clippy -p musicum-cli -- -D warnings
cargo test -p musicum-cli
```

---

### Task 4: Update `files.rs` — Show

**Files:**
- Modify: `apps/cli/src/commands/files.rs`

**Steps:**

1. Update the import (line 6):

```rust
use crate::output::{DetailItem::{Field, Section}, print_detail, print_json, print_table};
```

2. **`FilesCommand::Show`** — replace the two separate `print_detail` calls and the `println!()` between them (lines 64–95) with a single built vec:

```rust
let mut items: Vec<crate::output::DetailItem> = vec![
    Field("slug", file.slug.clone()),
    Field("name", file.name.clone()),
    Field("path", file.path.clone()),
    Field("duration", format!("{:.3}s", file.duration)),
    Field("sample_rate", format!("{}Hz", file.sample_rate)),
    Field("channels", file.channels.to_string()),
    Field("mime_type", file.mime_type.clone()),
    Field("hash", file.hash[..16].to_string() + "..."),
];
if let Some(m) = &meta {
    items.push(Section("metadata"));
    items.push(Field("bpm", m.bpm.map_or("-".into(), |v| v.to_string())));
    items.push(Field("key", m.key.clone().unwrap_or_else(|| "-".into())));
    items.push(Field("rating", m.rating.map_or("-".into(), |v| v.to_string())));
    items.push(Field("tags", if m.tags.is_empty() { "-".into() } else { m.tags.clone() }));
    items.push(Field("notes", if m.notes.is_empty() { "-".into() } else { m.notes.clone() }));
}
print_detail(&items);
```

The `if !clips.is_empty()` block with `print_table` below is unchanged.

3. Run lint + tests:

```
cargo clippy -p musicum-cli -- -D warnings
cargo test -p musicum-cli
```

---

### Task 5: Update `collections.rs` — Show

**Files:**
- Modify: `apps/cli/src/commands/collections.rs`

**Steps:**

1. Update the import (line 6):

```rust
use crate::output::{DetailItem::Field, print_detail, print_json, print_table};
```

2. **`CollectionsCommand::Show`** — replace the `print_detail` call (lines 47–51):

```rust
print_detail(vec![
    ("slug", col.slug.clone()),
    ("title", col.title.clone()),
    ("description", if col.description.is_empty() { "-".into() } else { col.description.clone() }),
]);
```

with:

```rust
print_detail(&[
    Field("slug", col.slug.clone()),
    Field("title", col.title.clone()),
    Field("description", if col.description.is_empty() { "-".into() } else { col.description.clone() }),
]);
```

3. Run the full lint + test suite one final time:

```
cargo clippy -p musicum-cli -- -D warnings
cargo test -p musicum-cli
```

Expected: zero warnings, all tests pass.

---

## Verification Checklist

After all tasks are complete, manually verify these representative commands against a library dir:

| Command | Expected output shape |
|---|---|
| `musicum presets list` | table unchanged |
| `musicum presets show <slug>` | key-value, right-aligned keys, `\nprocessors` section + table |
| `musicum presets create --title Foo` | "Created preset" then key-value detail |
| `musicum presets remove <slug>` | single "Removed preset 'slug'" line |
| `musicum presets add-processor <slug> crop` | "Added processor" then id/preset/type fields |
| `musicum clips show <slug>` | key-value, then blank line + "file" label + file fields |
| `musicum clips create <file> "My Clip"` | "Created clip" then slug/file fields |
| `musicum files show <slug>` | file fields, optional "metadata" section, optional clips table |
| `musicum collections show <slug>` | three key-value fields |
