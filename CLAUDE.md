# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
# Develop with live reload (serves on http://localhost:8080)
trunk serve

# Production build (output in dist/)
trunk build --release

# Format
cargo fmt

# Lint (target must be specified — this is WASM-only)
cargo clippy --target wasm32-unknown-unknown -- -D warnings

# Run native unit tests
cargo test --lib

The pre-commit hook (`.githooks/pre-commit`) runs `cargo fmt` then `cargo clippy`. Activate it once per clone:

```sh
git config core.hooksPath .githooks
```

## Tests

```sh
# Native unit tests (no WASM toolchain needed)
cargo test --lib

# WASM integration tests (requires Chrome or Firefox)
wasm-pack test --headless --chrome
```

### Current coverage (8 native + 3 WASM tests, all in `src/time.rs`)

| Test | Kind | What it checks |
|------|------|----------------|
| `empty_events_all_zero` | native | `event_row_counts` returns all zeros for empty input |
| `event_in_today_counts_all_periods` | native | event at now counts in today, week, month, total |
| `event_yesterday_not_today_but_in_week_and_month` | native | 24 h old event skips today, hits week + month |
| `event_eight_days_ago_only_in_month` | native | >7 days old skips week, still in month |
| `event_before_month_start_only_in_total` | native | event before month start only hits total |
| `mixed_events_correct_counts` | native | combination of all boundary cases |
| `topic_header_serde_round_trip` | native | `TopicHeader` serialises and deserialises correctly |
| `event_row_serde_round_trip` | native | `EventRow` serialises and deserialises correctly |
| `parse_valid_import_line` | WASM | valid timestamp line parses to an `EventRow` |
| `parse_empty_import_line_returns_none` | WASM | empty / blank lines return `None` |
| `parse_malformed_import_line_returns_none` | WASM | bad input returns `None` |

> **Note:** `time_boundaries()` and `format_timestamp()` are not unit-tested — they depend on `js_sys::Date` and require the WASM runtime. Covered manually.

## Source modules

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point — mounts the Leptos app |
| `src/app.rs` | All UI components and signal wiring |
| `src/db.rs` | IndexedDB access via `rexie` |
| `src/time.rs` | Timestamp helpers, count computation, import/export |
| `src/lib.rs` | Re-exports for the `trackitlib` rlib crate |

## Key dependencies

| Crate | Version | Role |
|-------|---------|------|
| `leptos` | 0.8 (CSR) | Reactive UI framework |
| `rexie` | 0.5 | IndexedDB async wrapper |
| `serde` / `serde_json` | 1.0 | Serialisation |
| `wasm-bindgen` / `js-sys` / `web-sys` | latest | WASM ↔ JS bridge |

Rust edition: **2024**.

## Architecture

Single-page PWA built with **Leptos** (CSR/WASM) and **Trunk**. Entry point is `src/main.rs`; logic is split across modules (`src/app.rs`, `src/db.rs`, etc.).

### Data layer

Persistence is **IndexedDB** via `rexie`, accessed through a thread-local handle:

```
thread_local! { static DB: RefCell<Option<Rexie>> }
```

Two IDB stores:
- `topics` — keyed by `id`, holds `TopicHeader` (name + pre-computed counts)
- `events` — keyed by `id`, indexed by `topic_id` via `by_topic`, holds `EventRow`

Counts (today / week / month / total) are stored **denormalized** in `TopicHeader` and recomputed from `EventRow` timestamps whenever events are added, deleted, or imported.

### Reactive model

Leptos signals are the only state:
- `TopicList` = `RwSignal<Vec<RwSignal<TopicHeader>>>` — outer signal changes on add/remove, inner signals change when counts change (avoids full list re-renders)
- Three newtype-wrapped `RwSignal<bool>` passed via Leptos context: `Editing`, `ShowDetail`, `DbReady`

IDB calls always happen inside `spawn_local` (async on the WASM event loop).

### Import / export format

Plain-text, one timestamp per line: `YYYY-MM-DD HH:MM:SS.mmm000`. Import parses via `js_sys::Date`; export triggers a browser download via a `Blob` URL.

### PWA

`index.html` → `manifest.json` + `public/service-worker.js` + `public/icon.svg`. Trunk copies these into `dist/` at build time (`copy-file` / `copy-dir` directives in `index.html`).

## Self-Maintenance Rule

After every major change, update this file to reflect the current state. Specifically:

- **New module** — add a row to the "Source modules" table
- **New dependency** — add a row to the "Key dependencies" table
- **Architectural shift** — update the Architecture section
- **New IDB store or index** — update the Data layer section
- **New context value or signal type** — update the Reactive model section
- **Build / tooling change** — update the Commands section and dependency versions
- **Tests added or removed** — update the Tests section (count, how to run, what is covered)
- **Gotchas discovered** — add a "Gotchas & Pitfalls" section if one does not exist

Keep this file as the single source of truth for AI sessions working on this project.
