# Collections CLI — Design Spec

**Date:** 2026-05-29

## Overview

Add full collection management to the CLI: create collections, add/remove clips with
explicit position control, list collections, and show collection details including
ordered clip membership.

The `collection` and `collection_clip` DB entities already exist. The `list` and `show`
(metadata-only) commands already exist. This spec extends both the service layer and CLI
to cover the missing operations.

---

## CLI commands

```
musicum collections create <title> [--description <desc>]
musicum collections set-description <slug> <description>
musicum collections add-clip <collection-slug> <clip-slug> [--position N]
musicum collections remove-clip <collection-slug> <clip-slug>
musicum collections delete <slug>
musicum collections list [--json]
musicum collections show <slug> [--json]
```

### `create <title> [--description <desc>]`

- Slugifies `<title>` (same pattern as `clips create`).
- Stores a new row in `collection` with a UUID id, the derived slug, title, optional
  description (defaults to empty string), and `created_at`/`updated_at` timestamps.
- Returns `InvalidInput` if the slug already exists.
- Output: `print_result("Created collection", [slug, title])`.

### `set-description <slug> <description>`

- Resolves collection by slug. Fails with `NotFound` if it doesn't exist.
- Replaces the description field (full replace, same pattern as `clips set-notes`).
- Updates `updated_at`.
- Output: `print_result("Updated collection", [slug])`.

### `delete <slug>`

- Resolves collection by slug. Fails with `NotFound` if it doesn't exist.
- Deletes all `collection_clip` rows for this collection, then deletes the `collection` row.
- Output: `print_result("Deleted collection", [slug])`.

### `add-clip <collection-slug> <clip-slug> [--position N]`

- Resolves both slugs to DB IDs. Fails with `NotFound` if either doesn't exist.
- Fails with `InvalidInput` if the clip is already a member of the collection.
- If `--position` is omitted: appends to end (`position = current count + 1`).
- If `--position N` is given (1-based): shifts all existing rows with `position >= N` up
  by 1, then inserts the new row at position N.
- Output: `print_result("Added clip", [collection, clip, position])`.

### `remove-clip <collection-slug> <clip-slug>`

- Resolves both slugs. Fails with `NotFound` if the clip is not a member.
- Deletes the `collection_clip` join row.
- Renumbers remaining rows contiguously (1-based, ordered by current position).
- Output: `print_result("Removed clip", [collection, clip])`.

### `list [--json]`

Unchanged from existing implementation. Table with `SLUG` and `TITLE` columns.

### `show <slug> [--json]`

Extended from existing implementation. Human-readable output:

```
── collection ──────────────────────
     slug: my-collection
    title: My Collection
     desc: Some description

── clips ───────────────────────────
#  SLUG               TITLE
1  ambient-intro      Ambient Intro
2  drop-section       Drop Section
```

JSON output returns a single object: `{ "collection": {...}, "clips": [...] }`.

If the collection has no clips, the clips section shows `(none)` (same pattern as
`clips show` with no processors).

---

## Service layer

All changes in `libs/musicum-core/src/services/collection_service.rs`.

### New functions

**`create_collection(db, title, description) → Result<collection::Model, ServiceError>`**

Generates UUID id, slugifies title, checks for slug uniqueness, inserts row.

**`add_clip_to_collection(db, collection_slug, clip_slug, position: Option<i32>) → Result<collection_clip::Model, ServiceError>`**

1. Resolve `collection_slug` → `collection_id` (NotFound if missing).
2. Resolve `clip_slug` → `clip_id` (NotFound if missing).
3. Check the `(collection_id, clip_id)` pair doesn't already exist (InvalidInput if it does).
4. Compute position: if `None`, query `MAX(position)` for this collection + 1 (or 1 if empty).
5. If explicit position: `UPDATE collection_clip SET position = position + 1 WHERE collection_id = ? AND position >= N`.
6. Insert new `collection_clip` row.

**`remove_clip_from_collection(db, collection_slug, clip_slug) → Result<(), ServiceError>`**

1. Resolve both slugs to IDs.
2. Find and delete the join row (NotFound if not a member).
3. Re-fetch remaining rows ordered by position and update them to `1, 2, 3, ...`.

**`set_collection_description(db, slug, description) → Result<(), ServiceError>`**

Fetches collection by slug, updates the `description` and `updated_at` fields.

**`delete_collection(db, slug) → Result<(), ServiceError>`**

Fetches collection by slug, deletes all `collection_clip` rows for its id, then deletes
the `collection` row.

**`get_collection_with_clips(db, slug) → Result<(collection::Model, Vec<clip::Model>), ServiceError>`**

Fetches collection by slug, then joins `collection_clip` + `clip` ordered by
`collection_clip.position` to return the clip list in display order.

---

## Persistence

Collections are **DB-only** — no sidecar files. The DB is the source of truth for
collection membership and ordering. This differs from clips, which are sidecar-first.

---

## Error handling

Follows existing `ServiceError` variants:
- `NotFound` — collection or clip slug doesn't resolve.
- `InvalidInput` — slug collision on create, or duplicate clip on add-clip.
- `DbErr` — propagated from SeaORM.

CLI surfaces these as `anyhow::Error` with the existing error display path.

---

## Out of scope

- Reorder command (drag/set new position for existing member) — can be added later.
- `background_path` field on collection — not exposed in CLI.
