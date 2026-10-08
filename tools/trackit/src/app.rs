//! The `App` root: contexts, startup, `?add=` deep links and foreground refresh.

use crate::db::{
    DB, EventRow, TopicHeader, add_event_and_update_header_idb, count_topic_events_idb, get_db,
    load_events_for_topic, load_topic_headers, open_db, save_topic_header, save_topic_headers_idb,
};
use crate::event_detail::EventDetail;
use crate::import::{
    ImportOutcome, count_noun, import_into_topic, import_message, next_position, other_topic_names,
};
use crate::time::{
    NameError, export_all, new_id, now_timestamp, parse_bulk_import, parse_import_line,
    time_boundaries, validate_topic_name, with_added_event,
};
use crate::toasts::{ToastBar, Toasts};
use crate::topic_card::TopicCard;
use crate::topic_detail::TopicDetail;
use leptos::prelude::*;
use leptos::task::spawn_local;
use rexie::Rexie;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

pub(crate) const PAGE_SIZE: usize = 50;

// ─── Context newtypes ─────────────────────────────────────────────────────────

// Newtype wrappers so Leptos context lookup never confuses same-type signals.
#[derive(Clone, Copy)]
pub(crate) struct Editing(pub(crate) RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowDetail(pub(crate) RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowEventDetail(pub(crate) RwSignal<bool>);
/// Events of the topic open in the detail screen, shared with the event
/// detail screen so edits there show up in the list. `all` holds every event
/// (newest first, feeds the statistics); `page` is the visible prefix.
#[derive(Clone, Copy)]
pub(crate) struct DetailEvents {
    pub(crate) page: RwSignal<Vec<EventRow>>,
    pub(crate) all: RwSignal<Vec<EventRow>>,
}

impl DetailEvents {
    /// Replace the stored copy of `row` (matched by id), keep `all` sorted
    /// newest first and refresh the visible page.
    pub(crate) fn replace(self, row: &EventRow) {
        self.all.update(|v| {
            if let Some(e) = v.iter_mut().find(|e| e.id == row.id) {
                *e = row.clone();
            }
        });
        self.resort(0);
    }

    /// Add a newly logged event at its place in time (a manual entry can be
    /// in the past) and grow the visible page by one.
    pub(crate) fn insert(self, row: &EventRow) {
        self.all.update(|v| v.push(row.clone()));
        self.resort(1);
    }

    pub(crate) fn remove(self, event_id: &str) {
        self.all.update(|v| v.retain(|e| e.id != event_id));
        self.page.update(|v| v.retain(|e| e.id != event_id));
    }

    /// Show a GPS-enriched row in the list and, if it is open, in the event
    /// detail screen.
    pub(crate) fn show_enriched(self, open: RwSignal<Option<EventRow>>, row: &EventRow) {
        self.replace(row);
        if open.with_untracked(|e| e.as_ref().is_some_and(|e| e.id == row.id)) {
            open.set(Some(row.clone()));
        }
    }

    fn resort(self, grow_page: usize) {
        self.all
            .update(|v| v.sort_by(|a, b| b.timestamp_ms.total_cmp(&a.timestamp_ms)));
        let len = self.page.with_untracked(Vec::len) + grow_page;
        self.page
            .set(self.all.with_untracked(|v| v[..len.min(v.len())].to_vec()));
    }
}

/// Id of the topic whose "−" was tapped and now shows a "Delete" confirm button.
#[derive(Clone, Copy)]
pub(crate) struct PendingDelete(pub(crate) RwSignal<Option<String>>);

// Per-topic reactive signal list. Outer signal changes only on add/remove;
// inner RwSignal<TopicHeader> changes only when that topic's counts change.
pub(crate) type TopicList = RwSignal<Vec<RwSignal<TopicHeader>>>;

// ─── URL action helper ────────────────────────────────────────────────────────

/// Extract the raw (URL-encoded) value of the `add` query parameter.
/// Returns `None` if the parameter is absent.
/// Caller is responsible for decoding percent-encoding (e.g. via
/// `js_sys::decode_uri_component`) before using the value.
fn parse_add_param_raw(search: &str) -> Option<&str> {
    let s = search.strip_prefix('?').unwrap_or(search);
    for pair in s.split('&') {
        if let Some(val) = pair.strip_prefix("add=") {
            return Some(val);
        }
    }
    None
}

// ─── Topic signals ────────────────────────────────────────────────────────────

/// Create a topic's signal owned by `owner` (the `App`), not by whichever
/// reactive scope happens to be current. Inside an event handler that is the
/// handler's view scope, e.g. the `<Show when=adding>` form, which is disposed
/// as soon as it hides; the next access to the signal then panics.
pub(crate) fn new_topic_signal(owner: &Owner, h: TopicHeader) -> RwSignal<TopicHeader> {
    owner.with(|| RwSignal::new(h))
}

// ─── Foreground refresh helper ────────────────────────────────────────────────

/// Recount every topic (index counts, one transaction), update the signals
/// whose counts changed and persist only those headers.
/// Called right after startup and whenever the page returns to the foreground.
pub(crate) async fn refresh_all_topic_counts(db: &Rexie, topic_list: TopicList) {
    let sigs = topic_list.get_untracked();
    let ids: Vec<String> = sigs
        .iter()
        .map(|s| s.with_untracked(|h| h.id.clone()))
        .collect();
    // One readonly transaction of index counts; no events are loaded.
    let Some(counts) = count_topic_events_idb(db, &ids, time_boundaries()).await else {
        return;
    };
    let mut changed = Vec::new();
    for (sig, c) in sigs.iter().zip(counts) {
        if sig.with_untracked(|h| h.counts()) != c {
            sig.update(|h| h.set_counts(c));
            changed.push(sig.get_untracked());
        }
    }
    if !changed.is_empty() {
        save_topic_headers_idb(db, &changed).await;
    }
}

#[component]
pub fn App() -> impl IntoView {
    let topic_list: TopicList = RwSignal::new(Vec::new());
    let toasts = Toasts::new();
    // Copy handle to the App's owner, for creating topic signals from handlers.
    let app_owner = StoredValue::new_local(Owner::current().expect("App runs inside an owner"));
    let db_ready_signal = RwSignal::new(false);
    // Set when IndexedDB cannot be opened (e.g. private browsing, blocked storage).
    let db_error: RwSignal<Option<String>> = RwSignal::new(None);

    let (new_name, set_new_name) = signal(String::new());
    let editing = RwSignal::new(false);
    let adding = RwSignal::new(false);
    let show_detail: RwSignal<bool> = RwSignal::new(false);
    let detail_id: RwSignal<String> = RwSignal::new(String::new());
    let show_event_detail: RwSignal<bool> = RwSignal::new(false);
    let event_detail_ev: RwSignal<Option<EventRow>> = RwSignal::new(None);

    // Read ?add=<topic-name> synchronously before entering the async block.
    let pending_add: Option<String> = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .as_deref()
        .and_then(parse_add_param_raw)
        .and_then(|raw| {
            js_sys::decode_uri_component(raw)
                .ok()
                .and_then(|v| v.as_string())
                .filter(|s| !s.is_empty())
        });

    spawn_local(async move {
        let db = match open_db().await {
            Ok(db) => db,
            Err(e) => {
                leptos::logging::error!("IndexedDB open failed: {e:?}");
                db_error.set(Some(e.to_string()));
                return;
            }
        };
        // Show the stored headers right away; counts are refreshed below,
        // after the list is on screen.
        let mut headers = load_topic_headers(&db).await;

        // ── Handle ?add=<topic-name> ─────────────────────────────────────────
        if let Some(ref name) = pending_add {
            if let Some(header) = headers.iter_mut().find(|h| &h.name == name) {
                let row = EventRow {
                    id: new_id(),
                    topic_id: header.id.clone(),
                    timestamp: now_timestamp(),
                    timestamp_ms: js_sys::Date::now(),
                    ..Default::default()
                };
                let updated = with_added_event(header, row.timestamp_ms, time_boundaries());
                if add_event_and_update_header_idb(&db, &row, &updated).await {
                    *header = updated;
                }
            }
            // Remove param so a reload does not re-fire the action.
            if let Some(w) = web_sys::window()
                && let Ok(hist) = w.history()
            {
                let path = w.location().pathname().unwrap_or_else(|_| "/".into());
                let _ = hist.replace_state_with_url(&JsValue::NULL, "", Some(&path));
            }
        }

        DB.with(|cell| *cell.borrow_mut() = Some(std::rc::Rc::new(db)));
        topic_list.set(
            headers
                .into_iter()
                .map(|h| app_owner.with_value(|o| new_topic_signal(o, h)))
                .collect(),
        );
        db_ready_signal.set(true);

        // Stored counts may be stale (a day or more may have passed).
        if let Some(db) = get_db() {
            refresh_all_topic_counts(&db, topic_list).await;
        }
    });

    provide_context(topic_list);
    provide_context(toasts);
    provide_context(Editing(editing));
    provide_context(ShowDetail(show_detail));
    provide_context(detail_id);
    provide_context(ShowEventDetail(show_event_detail));
    let pending_delete = RwSignal::new(None::<String>);
    provide_context(PendingDelete(pending_delete));
    // Leaving edit mode cancels an armed delete.
    Effect::new(move |_| {
        if !editing.get() {
            pending_delete.set(None);
        }
    });
    // The Edit/Done button disappears with the last topic, so leave edit mode
    // too; otherwise a topic added next would ignore taps.
    Effect::new(move |_| {
        if topic_list.with(Vec::is_empty) {
            editing.set(false);
        }
    });
    provide_context(event_detail_ev);
    provide_context(DetailEvents {
        page: RwSignal::new(Vec::new()),
        all: RwSignal::new(Vec::new()),
    });

    // ── Foreground detection: recount whenever the page becomes visible ───────
    // Cheap (index counts only), and keeps the rolling 7-day count current too.
    {
        let doc = web_sys::window().unwrap().document().unwrap();
        let doc2 = doc.clone(); // moved into the closure

        let listener = Closure::<dyn Fn()>::new(move || {
            if doc2.hidden() {
                return; // fired while going to background — nothing to do
            }
            spawn_local(async move {
                let Some(db) = get_db() else { return };
                refresh_all_topic_counts(&db, topic_list).await;
            });
        });

        doc.add_event_listener_with_callback("visibilitychange", listener.as_ref().unchecked_ref())
            .unwrap();
        // The App component lives for the entire page lifetime, so the
        // listener should too. Leaking avoids the Send + Sync requirement
        // that on_cleanup imposes on its closure.
        listener.forget();
    }

    let on_import = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = ev.target().unwrap().dyn_into().unwrap();
        let files = input.files().unwrap();
        if files.length() == 0 {
            return;
        }
        let file = files.get(0).unwrap();

        let filename = file.name();
        let topic_name = filename
            .strip_suffix(".txt")
            .unwrap_or(&filename)
            .to_string();

        let reader = web_sys::FileReader::new().unwrap();
        let reader_clone = reader.clone();

        let on_load = Closure::once(move |_: JsValue| {
            let text = reader_clone.result().unwrap().as_string().unwrap();
            let new_rows: Vec<EventRow> = text.lines().filter_map(parse_import_line).collect();
            let Some(db) = get_db() else { return };
            spawn_local(async move {
                let scope = format!("into {topic_name}");
                match import_into_topic(&db, topic_list, app_owner, topic_name, new_rows).await {
                    Some(outcome) => toasts.show(import_message(&scope, outcome)),
                    None => toasts.show("Import failed"),
                }
            });
        });

        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        reader.read_as_text(&file).unwrap();
        input.set_value("");
    };

    // ── Bulk JSON export ──────────────────────────────────────────────────────
    let on_export_all = move |_: leptos::ev::MouseEvent| {
        spawn_local(async move {
            let Some(db) = get_db() else { return };
            let headers = load_topic_headers(&db).await;
            let mut topic_events: Vec<(TopicHeader, Vec<EventRow>)> = Vec::new();
            for h in headers {
                let events = load_events_for_topic(&db, &h.id).await;
                topic_events.push((h, events));
            }
            export_all(&topic_events);
        });
    };

    // ── Bulk JSON import ──────────────────────────────────────────────────────
    let on_import_json = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = ev.target().unwrap().dyn_into().unwrap();
        let files = input.files().unwrap();
        if files.length() == 0 {
            return;
        }
        let file = files.get(0).unwrap();
        let reader = web_sys::FileReader::new().unwrap();
        let reader_clone = reader.clone();

        let on_load = Closure::once(move |_: JsValue| {
            let text = reader_clone.result().unwrap().as_string().unwrap();
            let Some(bulk) = parse_bulk_import(&text) else {
                toasts.show("Not a trackit backup");
                return;
            };
            let Some(db) = get_db() else { return };

            spawn_local(async move {
                let topics = bulk.topics.len();
                let mut total = ImportOutcome::default();
                let mut failed = false;
                for topic_export in bulk.topics {
                    match import_into_topic(
                        &db,
                        topic_list,
                        app_owner,
                        topic_export.name,
                        topic_export.events,
                    )
                    .await
                    {
                        Some(outcome) => total = total + outcome,
                        None => failed = true,
                    }
                }
                let scope = format!("from {}", count_noun(topics, "topic"));
                let mut msg = import_message(&scope, total);
                if failed {
                    msg.push_str(" – some topics failed");
                }
                toasts.show(msg);
            });
        });

        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        reader.read_as_text(&file).unwrap();
        input.set_value("");
    };

    let add_topic = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let name = match validate_topic_name(&new_name.get(), &other_topic_names(topic_list, "")) {
            Ok(name) => name,
            Err(NameError::Empty) => return,
            Err(e) => {
                toasts.show(e.message());
                return;
            }
        };
        let header = TopicHeader {
            position: next_position(topic_list),
            ..TopicHeader::new(new_id(), name)
        };
        if let Some(db) = get_db() {
            let h2 = header.clone();
            spawn_local(async move {
                save_topic_header(&db, &h2).await;
            });
        }
        let sig = app_owner.with_value(|o| new_topic_signal(o, header));
        topic_list.update(|rows| rows.push(sig));
        set_new_name.set(String::new());
        adding.set(false);
    };

    view! {
        <div class="app">
            // ── Overview screen ───────────────────────────────────────────────
            <div
                class="screen screen-overview"
                class:pushed=move || show_detail.get()
            >
                <header class="app-header">
                    <div class="header-bar">
                        <Show
                            when=move || !topic_list.get().is_empty()
                            fallback=|| view! { <div class="header-btn header-btn-left"></div> }
                        >
                            <button
                                class="header-btn header-btn-left"
                                on:click=move |_| editing.update(|e| *e = !*e)
                            >
                                {move || if editing.get() { "Done" } else { "Edit" }}
                            </button>
                        </Show>
                        <h1>"trackit"</h1>
                        <div class="header-right">
                            <button
                                class="header-btn"
                                type="button"
                                title="Export all topics (JSON)"
                                aria-label="Export all topics as JSON backup"
                                on:click=on_export_all
                            >
                                "⬇"
                            </button>
                            <label class="header-btn header-btn-import" title="Import all topics (JSON)">
                                "⬆"
                                <input type="file" accept=".json" class="visually-hidden" aria-label="Import JSON backup" on:change=on_import_json />
                            </label>
                            <label class="header-btn header-btn-import" title="Import topic from .txt">
                                "↑"
                                <input type="file" accept=".txt" class="visually-hidden" aria-label="Import topic from text file" on:change=on_import />
                            </label>
                            <button
                                class="header-btn header-btn-right"
                                type="button"
                                aria-label=move || if adding.get() { "Cancel adding topic" } else { "Add topic" }
                                on:click=move |_| {
                                    let now_adding = !adding.get();
                                    adding.set(now_adding);
                                    if !now_adding { set_new_name.set(String::new()); }
                                }
                            >
                                {move || if adding.get() { "Cancel" } else { "+" }}
                            </button>
                        </div>
                    </div>
                    <Show when=move || adding.get()>
                        <form class="add-topic-bar" on:submit=add_topic>
                            <input
                                class="topic-input"
                                type="text"
                                placeholder="New topic…"
                                prop:value=new_name
                                on:input=move |e| set_new_name.set(event_target_value(&e))
                            />
                            <button class="btn btn-add" type="submit">"Add"</button>
                        </form>
                    </Show>
                </header>
                <main class="app-main">
                    <div class="topic-list">
                        <Show when=move || !db_ready_signal.get() && db_error.with(Option::is_none)>
                            <div class="loading-indicator">"Loading…"</div>
                        </Show>
                        <Show when=move || db_error.with(Option::is_some)>
                            <div class="empty-state db-error">
                                <p>"Storage unavailable."</p>
                                <p>"trackit needs IndexedDB, which this browser has blocked (private mode?)."</p>
                                <p class="db-error-detail">{move || db_error.get().unwrap_or_default()}</p>
                            </div>
                        </Show>
                        <Show when=move || db_ready_signal.get() && topic_list.get().is_empty()>
                            <div class="empty-state">
                                <p>"No topics yet."</p>
                                <p>"Tap \"+\" to add one."</p>
                            </div>
                        </Show>
                        <For
                            each=move || topic_list.get()
                            key=|sig| sig.with_untracked(|h| h.id.clone())
                            children=|sig| view! { <TopicCard topic_signal=sig /> }
                        />
                    </div>
                </main>
            </div>

            // ── Detail screen ─────────────────────────────────────────────────
            <div
                class="screen screen-detail"
                class:active=move || show_detail.get()
                class:pushed=move || show_event_detail.get()
            >
                <TopicDetail />
            </div>

            // ── Event detail screen ───────────────────────────────────────────
            <div
                class="screen screen-event-detail"
                class:active=move || show_event_detail.get()
            >
                <EventDetail />
            </div>

            <ToastBar />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{add_event_idb, open_db, save_topic_header};
    use crate::time::{new_id, now_timestamp};
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    wasm_bindgen_test_configure!(run_in_browser);

    /// Calling `refresh_all_topic_counts` must replace stale denormalized
    /// counts with the values recomputed from the stored events.
    #[wasm_bindgen_test]
    async fn refresh_all_counts_corrects_stale_signal() {
        let db = open_db().await.unwrap();

        let topic_id = new_id();
        let header = TopicHeader {
            id: topic_id.clone(),
            name: "stale-test".into(),
            count_today: 99,
            count_week: 99,
            count_month: 99,
            position: 0,
            count_total: 99,
        };
        save_topic_header(&db, &header).await;

        let row = EventRow {
            id: new_id(),
            topic_id: topic_id.clone(),
            timestamp: now_timestamp(),
            timestamp_ms: js_sys::Date::now(),
            ..Default::default()
        };
        add_event_idb(&db, &row).await;

        let sig: RwSignal<TopicHeader> = RwSignal::new(header);
        let topic_list: TopicList = RwSignal::new(vec![sig]);

        refresh_all_topic_counts(&db, topic_list).await;

        let h = sig.get_untracked();
        assert_eq!(h.count_total, 1, "total should be 1 after refresh");
        assert_ne!(h.count_today, 99, "today should not be stale 99");
    }

    /// A topic added from the `<Show when=adding>` form must survive that
    /// form's scope being disposed when the form hides.
    #[wasm_bindgen_test]
    fn topic_signal_outlives_disposed_handler_scope() {
        let app = Owner::new();
        let sig = app.with(|| {
            let form_scope = Owner::new();
            let sig = form_scope
                .with(|| new_topic_signal(&app, TopicHeader::new("t".into(), "Running".into())));
            form_scope.cleanup();
            sig
        });
        assert!(
            sig.try_get_untracked().is_some(),
            "topic signal was disposed with the handler scope"
        );
    }

    #[test]
    fn parse_add_param_raw_present() {
        assert_eq!(parse_add_param_raw("?add=Running"), Some("Running"));
        assert_eq!(parse_add_param_raw("?foo=bar&add=Cycling"), Some("Cycling"));
        assert_eq!(
            parse_add_param_raw("?add=Morning%20Run"),
            Some("Morning%20Run")
        );
    }

    #[test]
    fn parse_add_param_raw_absent() {
        assert_eq!(parse_add_param_raw(""), None);
        assert_eq!(parse_add_param_raw("?foo=bar"), None);
        assert_eq!(parse_add_param_raw("?adding=foo"), None); // prefix must be exact key
    }
}
