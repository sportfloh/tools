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

### Current coverage (9 native + 21 WASM tests)

| Test | Kind | Where | What it checks |
|------|------|-------|----------------|
| `empty_events_all_zero` | native | `time.rs` | `event_row_counts` returns all zeros for empty input |
| `event_in_today_counts_all_periods` | native | `time.rs` | event at now counts in today, week, month, total |
| `event_yesterday_not_today_but_in_week_and_month` | native | `time.rs` | 24 h old event skips today, hits week + month |
| `event_eight_days_ago_only_in_month` | native | `time.rs` | >7 days old skips week, still in month |
| `event_before_month_start_only_in_total` | native | `time.rs` | event before month start only hits total |
| `mixed_events_correct_counts` | native | `time.rs` | combination of all boundary cases |
| `stale_boundaries_miscount_crossed_day` | native | `time.rs` | same event counted as today yesterday is NOT today with current bounds |
| `topic_header_serde_round_trip` | native | `time.rs` | `TopicHeader` serialises and deserialises correctly |
| `event_row_serde_round_trip` | native | `time.rs` | `EventRow` serialises and deserialises correctly |
| `parse_valid_import_line` | WASM | `time.rs` | valid timestamp line parses to an `EventRow` |
| `parse_empty_import_line_returns_none` | WASM | `time.rs` | empty / blank lines return `None` |
| `parse_malformed_import_line_returns_none` | WASM | `time.rs` | bad input returns `None` |
| `format_timestamp_has_expected_shape` | WASM | `time.rs` | output is 21-char `DD.MM.YYYY - HH:MM:SS` |
| `format_timestamp_shape_is_stable_for_any_valid_iso` | WASM | `time.rs` | shape holds for a second ISO input |
| `now_timestamp_returns_iso_utc_string` | WASM | `time.rs` | non-empty, contains `T`, ends with `Z` |
| `now_local_datetime_str_has_expected_shape` | WASM | `time.rs` | 19-char `YYYY-MM-DDTHH:MM:SS` layout |
| `new_id_has_numeric_dash_numeric_format` | WASM | `time.rs` | ID is `{digits}-{digits}` |
| `new_id_is_unique_across_calls` | WASM | `time.rs` | two consecutive calls differ |
| `time_boundaries_ordering_invariants` | WASM | `time.rs` | `today_start ≤ now < today_end`, `month_start ≤ now`, `week_start ≤ now` |
| `time_boundaries_today_span_is_exactly_one_day` | WASM | `time.rs` | `today_end − today_start == 86 400 000` |
| `time_boundaries_week_start_is_seven_days_before_now` | WASM | `time.rs` | `now − week_start == 7 × 86 400 000` |
| `export_topic_does_not_panic` | WASM | `time.rs` | smoke test: empty slice + one-event slice don't panic |
| `idb_save_and_load_topic` | WASM | `db.rs` | save a `TopicHeader`, reload, verify presence |
| `idb_add_and_load_events` | WASM | `db.rs` | add event, load by topic ID, verify it exists |
| `idb_delete_event` | WASM | `db.rs` | add event, delete it, verify not found |
| `idb_delete_topic_cascades` | WASM | `db.rs` | deleting a topic removes its header and all its events |
| `idb_save_topic_header_overwrites` | WASM | `db.rs` | saving same topic ID twice overwrites counts (upsert) |
| `idb_load_events_sorted_descending` | WASM | `db.rs` | `load_events_for_topic` returns events newest-first |
| `idb_refresh_topic_counts` | WASM | `db.rs` | `refresh_topic_counts_idb` replaces stale counts with correct recomputed values |
| `refresh_all_counts_corrects_stale_signal` | WASM | `app.rs` | `refresh_all_topic_counts` updates stale Leptos signals to match recomputed IDB counts |

## TDD Workflow

Every new feature **must** follow the red → green → refactor cycle:

1. **Red** — Write a failing test that names the desired behaviour. Commit it
   alone (or together with a stub that makes it compile but still fail at
   runtime). Run the suite and confirm the new test fails and only the new
   test fails:
   ```sh
   cargo test --lib                    # native tests
   wasm-pack test --headless --chrome  # WASM tests
   ```

2. **Green** — Write the *minimum* production code to make the failing test
   pass. Do not polish or generalise yet. Confirm the full suite is still
   green.

3. **Refactor** — Clean up both production code and the test while keeping the
   suite green.

### Choosing the right test kind

| Logic touches… | Use |
|---|---|
| Pure Rust (no JS APIs, no `web-sys`) | `#[test]` in the relevant module (`time.rs`, etc.) — runs with `cargo test --lib` |
| JS APIs / `web-sys` / IndexedDB | `#[wasm_bindgen_test]` in the relevant module — runs with `wasm-pack test` |

Prefer native tests wherever possible; they are faster and need no browser.

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
| `wasm-bindgen-futures` | 0.4 | Await JS Promises from async Rust (used for Geolocation) |

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
- Four newtype-wrapped `RwSignal<bool>` passed via Leptos context: `Editing`, `ShowDetail`, `ShowEventDetail`, `DbReady`
- `RwSignal<Option<EventRow>>` in context holds the event currently open in the event detail screen

IDB calls always happen inside `spawn_local` (async on the WASM event loop).

A `visibilitychange` listener is registered on `document` inside `App()`. When the page returns to the foreground (`!document.hidden`) and the `today_start` boundary has changed (i.e. midnight passed while the app was backgrounded), `refresh_all_topic_counts` is called to recompute and update every topic's counts from IDB. The `Closure` is intentionally leaked (`.forget()`) because the `App` component lives for the entire page lifetime.

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
- **New feature** — write the failing test first (red), then implement (green), then update the coverage table in the Tests section

Keep this file as the single source of truth for AI sessions working on this project.
