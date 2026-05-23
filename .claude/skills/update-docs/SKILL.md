---
name: update-docs
description: >
  Use this skill to create, audit, or update a CLAUDE.md file in any repository so it
  effectively guides Claude Code sessions. Trigger whenever the user mentions "CLAUDE.md",
  "Claude Code memory", "project context file", "update my CLAUDE.md", "generate a
  CLAUDE.md", "my Claude Code doesn't know about X", or any request to improve how
  Claude Code understands their project. Also trigger proactively when you're operating
  inside a repo that has no CLAUDE.md, or one that is clearly stale or bloated.
---
 
# CLAUDE.md Updater
 
A skill for creating and maintaining high-signal `CLAUDE.md` files that make Claude Code
sessions faster, more accurate, and require less repetitive context-setting.
 
---
 
## Why CLAUDE.md matters
 
Claude Code is stateless. Every session starts with zero knowledge of the codebase.
`CLAUDE.md` is the **only file** that automatically loads into every session — it is the
single highest-leverage configuration point for the entire tool.
 
A bad `CLAUDE.md` (too long, too generic, stale) actively hurts performance: the model
begins ignoring all instructions uniformly as instruction count grows. A good one acts
like a senior engineer's onboarding brief.
 
---
 
## Step 1 — Understand the repo before writing anything
 
Run these discovery commands before drafting or editing:
 
```bash
# Project structure (top 2 levels)
find . -maxdepth 2 -not -path '*/.git/*' -not -path '*/node_modules/*' \
  -not -path '*/__pycache__/*' | sort
 
# Package / dependency files
ls package.json pyproject.toml Cargo.toml go.mod composer.json Gemfile 2>/dev/null
 
# Existing scripts / Makefiles
cat package.json 2>/dev/null | python3 -m json.tool | grep -A1 '"scripts"' || true
cat Makefile 2>/dev/null | grep '^[a-zA-Z]' | head -20 || true
 
# Existing CLAUDE.md (read before touching)
cat CLAUDE.md 2>/dev/null || echo "(none)"
 
# Check for agent_docs or similar supplement directories
ls agent_docs/ docs/ .claude/ 2>/dev/null || true
```
 
Read any existing `CLAUDE.md` carefully — preserve valid content and only update what
is stale, missing, or wrong.
 
---
 
## Step 2 — Draft or update the file
 
### Structure (in order)
 
A high-quality `CLAUDE.md` follows this structure. Use only the sections that apply —
**omit blank sections entirely**.
 
```markdown
# CLAUDE.md
 
## Project overview
One paragraph: what the project is, what problem it solves, its current stage.
 
## Repo layout
Brief map of top-level directories. Especially important in monorepos.
Only list dirs Claude will actually need to touch.
 
## Tech stack
Language + version, framework, database, key libraries. One line each.
 
## Dev commands
<!-- Only universally applicable commands — things Claude needs in EVERY session -->
- `<build command>`: <what it does>
- `<test command>`: <what it does>
- `<lint/typecheck command>`: run before finishing any change
- `<dev server>`: how to start it
 
## Workflow conventions
Branch strategy, PR requirements, commit style — only if non-standard.
E.g. "Never commit directly to main", "Use conventional commits".
 
## Gotchas & known issues
Warnings that prevent wasted effort:
- "Don't run X in production — it wipes the DB"
- "Module Y must be built before Z or imports will fail"
- "Use bun, not npm/yarn — lockfile will break otherwise"
 
## Supplemental docs
<!-- Use progressive disclosure — keep details OUT of this file -->
When working on specific areas, read the relevant doc first:
- `agent_docs/architecture.md` — system design and service boundaries
- `agent_docs/testing.md` — test strategy, fixtures, mocking conventions
- `agent_docs/api.md` — API contracts and auth patterns
```
 
### Hard rules for writing the file
 
| Rule | Rationale |
|------|-----------|
| **< 80 lines total** (aim for < 60) | Instruction-following degrades uniformly as count rises |
| **No generic instructions** ("write clean code", "follow best practices") | Claude knows this already; it wastes instruction budget |
| **No code style rules** | Use a linter (ESLint, Biome, Ruff, etc.) — never an LLM |
| **No code snippets** | They go stale; use file references instead (`see src/auth/index.ts:42`) |
| **No task-specific instructions** | Only include what's needed in **every** session |
| **Prefer pointers over copies** | `see agent_docs/db.md` beats duplicating 40 lines here |
| **Only `CLAUDE.local.md` for personal prefs** | Don't commit personal workflow preferences to git |
 
---
 
## Step 3 — Create supplemental docs (if needed)
 
If the project is large, move non-universal instructions to `agent_docs/`:
 
```bash
mkdir -p agent_docs
```
 
Create focused `.md` files there and reference them from the `## Supplemental docs`
section in `CLAUDE.md`. Good candidates for separate files:
 
- Architecture / service map
- Testing conventions and how to run specific test suites  
- Database schema overview and migration workflow
- API contracts / auth
- Deployment process
- Onboarding / environment setup
This is **progressive disclosure**: Claude only loads these when working on the
relevant area, keeping the base context lean.
 
---
 
## Step 4 — Validate before saving
 
Run this mental checklist on the draft:
 
```
[ ] < 80 lines?
[ ] Does every line apply to EVERY Claude Code session in this repo?
[ ] No code style / formatting rules (those belong in a linter config)?
[ ] No code snippets that will go stale?
[ ] Dev commands are accurate and tested?
[ ] Repo layout matches current structure?
[ ] Gotchas are real, non-obvious, and would save time?
[ ] Supplemental doc pointers are accurate?
```
 
If any item fails, fix it before writing the file.
 
---
 
## Step 5 — Write the file
 
```bash
# Backup existing file first
cp CLAUDE.md CLAUDE.md.bak 2>/dev/null || true
 
# Write the new file
cat > CLAUDE.md << 'EOF'
<content>
EOF
```
 
Then show the user the diff:
 
```bash
diff CLAUDE.md.bak CLAUDE.md 2>/dev/null || echo "(new file)"
```
 
---
 
## Ongoing maintenance triggers
 
Remind the user to update `CLAUDE.md` when:
 
- A new primary dev command is added (build, test, lint)
- The repo structure changes significantly (new service, monorepo split)
- A non-obvious gotcha burns time in a session
- A dependency or runtime version changes
- A supplemental doc in `agent_docs/` is created or significantly changed
**Anti-pattern to avoid**: treating `CLAUDE.md` as a changelog or dumping ground for
every session's learnings. It should stay lean. One-off learnings → `agent_docs/`.
 
---
 
## Quick reference: what goes where
 
| Content | Location |
|---------|----------|
| Universal dev commands | `CLAUDE.md` |
| Repo map & tech stack | `CLAUDE.md` |
| Critical gotchas | `CLAUDE.md` |
| Architecture details | `agent_docs/architecture.md` |
| Testing deep-dive | `agent_docs/testing.md` |
| Task tracking / checklists | `docs/` with `[ ]` checkboxes |
| Personal preferences | `CLAUDE.local.md` (gitignored) |
| Code style rules | `.eslintrc` / `ruff.toml` / `biome.json` etc. |