# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
# Develop trackit with live reload (serves on http://localhost:8080)
cd tools/trackit && trunk serve

# Production build for trackit (output in tools/trackit/dist/)
cd tools/trackit && trunk build --release

# Format (workspace root — applies to all tools)
cargo fmt

# Lint (workspace root — target must be specified, all tools are WASM-only)
cargo clippy --target wasm32-unknown-unknown -- -D warnings

# Run native unit tests (workspace root)
cargo test --lib
```

The pre-commit hook (`.githooks/pre-commit`) runs `cargo fmt` then `cargo clippy`. Activate it once per clone:

```sh
git config core.hooksPath .githooks
```

## Tests

```sh
# Native unit tests (workspace root — no WASM toolchain needed)
cargo test --lib

# WASM integration tests (from the tool directory — requires Chrome or Firefox)
cd tools/trackit && wasm-pack test --headless --chrome
```

### Current coverage (14 native + 21 WASM tests)

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
| `bulk_export_serde_round_trip` | native | `time.rs` | `BulkExport` + `TopicExport` round-trip through JSON |
| `parse_bulk_import_valid` | native | `time.rs` | valid JSON deserialises to `BulkExport` with correct fields |
| `parse_bulk_import_invalid_returns_none` | native | `time.rs` | malformed JSON and missing fields return `None` |
| `parse_add_param_raw_present` | native | `app.rs` | `?add=` param is extracted correctly, including encoded values |
| `parse_add_param_raw_absent` | native | `app.rs` | missing / non-matching params return `None` |
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

## Repository layout

This is a Cargo workspace monorepo. Each tool lives under `tools/<name>/` and
is served at `https://<host>/tools/<name>/`.

```
Cargo.toml               ← workspace manifest + shared [profile.release]
Cargo.lock               ← workspace lock file
rust-toolchain.toml      ← stable + wasm32-unknown-unknown
tools/
└── trackit/             ← served at /tools/trackit/
    ├── Cargo.toml
    ├── Trunk.toml
    ├── index.html
    ├── manifest.json
    ├── src/
    ├── style/
    └── public/
```

To add a new tool: create `tools/<name>/` with its own `Cargo.toml` and
`Trunk.toml`, then add `"tools/<name>"` to the `members` list in the root
`Cargo.toml`.

## Source modules (trackit)

| File | Purpose |
|------|---------|
| `tools/trackit/src/main.rs` | Entry point — mounts the Leptos app |
| `tools/trackit/src/app.rs` | All UI components and signal wiring |
| `tools/trackit/src/db.rs` | IndexedDB access via `rexie` |
| `tools/trackit/src/time.rs` | Timestamp helpers, count computation, import/export |
| `tools/trackit/src/lib.rs` | Re-exports for the `trackitlib` rlib crate |

## Key dependencies

| Crate | Version | Role |
|-------|---------|------|
| `leptos` | 0.8 (CSR) | Reactive UI framework |
| `rexie` | 0.6 | IndexedDB async wrapper |
| `serde` / `serde_json` | 1.0 | Serialisation |
| `wasm-bindgen` / `js-sys` / `web-sys` | latest | WASM ↔ JS bridge |
| `wasm-bindgen-futures` | 0.4 | Await JS Promises from async Rust (used for Geolocation) |

Rust edition: **2024**.

## Architecture

Single-page PWA built with **Leptos** (CSR/WASM) and **Trunk**. Entry point is `tools/trackit/src/main.rs`; logic is split across modules (`src/app.rs`, `src/db.rs`, etc.).

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

**Per-topic (plain-text):** one timestamp per line: `YYYY-MM-DD HH:MM:SS.mmm000`. Import parses via `js_sys::Date`; export triggers a browser download via a `Blob` URL. Accessible via the `↑` / export button inside the topic detail screen.

**Bulk (JSON):** a single `trackit-YYYY-MM-DD.json` file containing all topics and all their events. Structure: `{ version: 1, topics: [{ id, name, events: [...EventRow] }] }`. Counts are excluded (recomputed on import). Accessible via the `⬇` (export) and `⬆` (import) buttons in the main header. Import is additive and deduplicates events by `timestamp` string, matching the per-topic import behaviour.

### URL actions (Apple Shortcuts / deep links)

The app reads the `?add=<topic-name>` query parameter **once on startup** (synchronously, before entering the async IDB block). If the named topic exists, a timestamped event (no GPS) is added to it; then `history.replaceState` strips the param so a page reload does not re-fire the action.

Use with Apple Shortcuts via the **"Open URLs"** action:
```
https://<host>/tools/?add=Running
https://<host>/tools/?add=Morning%20Run   ← spaces as %20
```
On iOS 16.4+ the PWA opens as a standalone app; the event is recorded immediately and the updated count is visible in the topic list. On older iOS versions the URL opens in Safari instead.

The parsing helper `parse_add_param_raw` is pure Rust (no WASM APIs) and is covered by native unit tests.

### PWA

`index.html` → `manifest.json` + `public/service-worker.js` + `public/icon.svg`. Trunk copies these into `dist/` at build time (`copy-file` / `copy-dir` directives in `index.html`).

## Git workflow

- **`main`** is the default and protected branch. All deployments to GitHub
  Pages happen automatically when a commit lands on `main` via a merged PR.
- **Never push directly to `main`.** All work happens on a short-lived
  `claude/<feature>-<session-id>` branch, then merges via a pull request.
- Branch names must start with `claude/` — the git proxy enforces this.

```
main  ←── PR merge ←── claude/<feature>-<session-id>
  │
  └── triggers deploy.yml → GitHub Pages
```

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
