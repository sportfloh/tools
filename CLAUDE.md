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
```

There are no tests. The pre-commit hook (`.githooks/pre-commit`) runs `cargo fmt` then `cargo clippy`. Activate it once per clone:

```sh
git config core.hooksPath .githooks
```

## Architecture

Single-page PWA built with **Leptos** (CSR/WASM) and **Trunk**. The entire application is `src/main.rs` — there are no modules.

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
