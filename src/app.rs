use crate::db::{
    DB, EventRow, TopicHeader, add_event_idb, delete_event_idb, delete_topic_idb, get_db,
    load_events_for_topic, load_topic_headers, open_db, save_topic_header,
};
use crate::time::{
    event_row_counts, export_topic, format_timestamp, new_id, now_local_datetime_str,
    now_timestamp, parse_import_line, time_boundaries,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
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
pub(crate) struct DbReady(pub(crate) RwSignal<bool>);

// Per-topic reactive signal list. Outer signal changes only on add/remove;
// inner RwSignal<TopicHeader> changes only when that topic's counts change.
pub(crate) type TopicList = RwSignal<Vec<RwSignal<TopicHeader>>>;

// ─── Components ──────────────────────────────────────────────────────────────

#[component]
pub fn TopicCard(topic_signal: RwSignal<TopicHeader>) -> impl IntoView {
    let topic_list = use_context::<TopicList>().expect("topic_list context");
    let db_ready = use_context::<DbReady>().expect("db_ready context").0;
    let editing = use_context::<Editing>().expect("editing context").0;
    let show_detail = use_context::<ShowDetail>().expect("show_detail context").0;
    let detail_id = use_context::<RwSignal<String>>().expect("detail_id context");

    let add_event = move |_| {
        let Some(db) = get_db() else { return };
        let _ = db_ready.get_untracked(); // just to acknowledge the signal
        let topic_id = topic_signal.with_untracked(|h| h.id.clone());
        let ts = now_timestamp();
        let ts_ms = js_sys::Date::now();
        let row = EventRow {
            id: new_id(),
            topic_id,
            timestamp: ts,
            timestamp_ms: ts_ms,
        };
        let row2 = row.clone();
        spawn_local(async move {
            add_event_idb(&db, &row2).await;
            let (now_ms, today_start, today_end, month_start, week_start) = time_boundaries();
            let ms = row2.timestamp_ms;
            topic_signal.update(|h| {
                h.count_total += 1;
                if ms >= today_start && ms < today_end {
                    h.count_today += 1;
                }
                if ms >= week_start && ms <= now_ms {
                    h.count_week += 1;
                }
                if ms >= month_start {
                    h.count_month += 1;
                }
            });
            save_topic_header(&db, &topic_signal.get_untracked()).await;
        });
    };

    let delete_topic = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        let id = topic_signal.with_untracked(|h| h.id.clone());
        let id2 = id.clone();
        if let Some(db) = get_db() {
            spawn_local(async move {
                delete_topic_idb(&db, &id).await;
            });
        }
        topic_list.update(|rows| rows.retain(|s| s.with_untracked(|h| h.id != id2)));
    };

    let go_detail = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        let id = topic_signal.with_untracked(|h| h.id.clone());
        detail_id.set(id);
        show_detail.set(true);
    };

    let topic_name = Memo::new(move |_| topic_signal.with(|h| h.name.clone()));
    let counts = Memo::new(move |_| {
        topic_signal.with(|h| (h.count_today, h.count_week, h.count_month, h.count_total))
    });

    view! {
        <div class="topic-row">
            <Show when=move || editing.get()>
                <button class="btn-delete-topic" on:click=delete_topic title="Delete topic">
                    "−"
                </button>
            </Show>
            <div class="topic-row-main" on:click=add_event>
                <span class="topic-row-name">{topic_name}</span>
                <span class="topic-row-counts">
                    {move || {
                        let (today, week, month, total) = counts.get();
                        format!("{} today · {} wk · {} mo · {} total", today, week, month, total)
                    }}
                </span>
            </div>
            <button class="btn-detail" on:click=go_detail title="Details">"›"</button>
        </div>
    }
}

#[component]
pub fn TopicDetail() -> impl IntoView {
    let topic_list = use_context::<TopicList>().expect("topic_list context");
    let show_detail = use_context::<ShowDetail>().expect("show_detail context").0;
    let detail_id = use_context::<RwSignal<String>>().expect("detail_id context");

    let show_add_modal: RwSignal<bool> = RwSignal::new(false);
    let manual_dt: RwSignal<String> = RwSignal::new(String::new());
    let swiped_id: RwSignal<Option<String>> = RwSignal::new(None);

    let events: RwSignal<Vec<EventRow>> = RwSignal::new(Vec::new());
    let loading: RwSignal<bool> = RwSignal::new(false);
    let page_end: RwSignal<usize> = RwSignal::new(PAGE_SIZE);
    let all_evs: StoredValue<Vec<EventRow>> = StoredValue::new(Vec::new());

    let go_back = move |_: leptos::ev::MouseEvent| {
        show_detail.set(false);
    };

    // Find the header signal for the currently-viewed topic.
    let current_header = Memo::new(move |_| {
        let id = detail_id.get();
        topic_list.with(|rows| {
            rows.iter()
                .find(|s| s.with_untracked(|h| h.id == id))
                .copied()
        })
    });

    let topic_name = Memo::new(move |_| {
        current_header
            .get()
            .map(|sig| sig.with(|h| h.name.clone()))
            .unwrap_or_default()
    });

    // Load events from IDB whenever the viewed topic changes.
    Effect::new(move |_| {
        let topic_id = detail_id.get();
        if topic_id.is_empty() {
            return;
        }
        let Some(db) = get_db() else { return };
        events.set(Vec::new());
        all_evs.set_value(Vec::new());
        page_end.set(PAGE_SIZE);
        loading.set(true);
        spawn_local(async move {
            let loaded = load_events_for_topic(&db, &topic_id).await;
            let page = loaded[..PAGE_SIZE.min(loaded.len())].to_vec();
            all_evs.set_value(loaded);
            events.set(page);
            loading.set(false);
        });
    });

    let load_more = move |_: leptos::ev::MouseEvent| {
        let next = page_end.get() + PAGE_SIZE;
        let slice = all_evs.with_value(|v| v[..next.min(v.len())].to_vec());
        events.set(slice);
        page_end.set(next);
    };

    let has_more = move || all_evs.with_value(|v| page_end.get() < v.len());

    let do_export = move |_: leptos::ev::MouseEvent| {
        let name = topic_name.get_untracked();
        all_evs.with_value(|v| export_topic(&name, v));
    };

    let open_add_modal = move |_: leptos::ev::MouseEvent| {
        manual_dt.set(now_local_datetime_str());
        show_add_modal.set(true);
    };
    let close_add_modal = move |_: leptos::ev::MouseEvent| {
        show_add_modal.set(false);
    };

    let add_manual_event = move |_: leptos::ev::MouseEvent| {
        let dt_str = manual_dt.get();
        let d = js_sys::Date::new(&JsValue::from_str(&dt_str));
        if !d.get_time().is_nan() {
            let iso = d.to_iso_string().as_string().unwrap_or_default();
            let ts_ms = d.get_time();
            let topic_id = detail_id.get_untracked();
            let row = EventRow {
                id: new_id(),
                topic_id,
                timestamp: iso,
                timestamp_ms: ts_ms,
            };
            // Optimistic UI update
            events.update(|evs| evs.insert(0, row.clone()));
            all_evs.update_value(|v| v.insert(0, row.clone()));
            if let Some(db) = get_db() {
                let row2 = row.clone();
                spawn_local(async move {
                    add_event_idb(&db, &row2).await;
                    let (now_ms, today_start, today_end, month_start, week_start) =
                        time_boundaries();
                    let ms = row2.timestamp_ms;
                    if let Some(sig) = current_header.get_untracked() {
                        sig.update(|h| {
                            h.count_total += 1;
                            if ms >= today_start && ms < today_end {
                                h.count_today += 1;
                            }
                            if ms >= week_start && ms <= now_ms {
                                h.count_week += 1;
                            }
                            if ms >= month_start {
                                h.count_month += 1;
                            }
                        });
                        save_topic_header(&db, &sig.get_untracked()).await;
                    }
                });
            }
        }
        show_add_modal.set(false);
    };

    // Swipe-back gesture (right edge → navigate back)
    let touch_start_x = StoredValue::new(0.0f64);
    let touch_start_y = StoredValue::new(0.0f64);
    let on_touch_start = move |ev: web_sys::TouchEvent| {
        if let Some(t) = ev.touches().get(0) {
            touch_start_x.set_value(t.client_x() as f64);
            touch_start_y.set_value(t.client_y() as f64);
        }
    };
    let on_touch_end = move |ev: web_sys::TouchEvent| {
        if let Some(t) = ev.changed_touches().get(0) {
            let sx = touch_start_x.get_value();
            let dx = t.client_x() as f64 - sx;
            let dy = (t.client_y() as f64 - touch_start_y.get_value()).abs();
            if sx < 40.0 && dx > 50.0 && dx > dy {
                show_detail.set(false);
            }
        }
    };

    view! {
        <div
            class="detail-wrapper"
            on:touchstart=on_touch_start
            on:touchend=on_touch_end
        >
            <header class="app-header">
                <div class="header-bar">
                    <button class="header-btn header-btn-back" on:click=go_back>"‹ Back"</button>
                    <h1>{topic_name}</h1>
                    <div class="header-right">
                        <button class="header-btn header-btn-right" on:click=do_export title="Export to .txt">"↓"</button>
                        <button class="header-btn header-btn-right" on:click=open_add_modal title="Log event manually">"+"</button>
                    </div>
                </div>
            </header>
            <main class="app-main app-main--detail">
                <div class="event-card">
                    <Show when=move || loading.get()>
                        <div class="loading-indicator">"Loading…"</div>
                    </Show>
                    <ul class="event-list" on:click=move |_| swiped_id.set(None)>
                        <Show when=move || !loading.get() && events.get().is_empty()>
                            <li class="event-empty">"No events yet — tap the topic row to log one."</li>
                        </Show>
                        <For
                            each=move || events.get()
                            key=|ev| ev.id.clone()
                            children=move |ev| {
                                let eid        = StoredValue::new(ev.id.clone());
                                let ts_str     = ev.timestamp.clone();
                                let swipe_tx_x = StoredValue::new(0.0f64);

                                let on_touch_start_row = move |te: web_sys::TouchEvent| {
                                    if let Some(t) = te.touches().get(0) {
                                        swipe_tx_x.set_value(t.client_x() as f64);
                                    }
                                };
                                let on_touch_end_row = move |te: web_sys::TouchEvent| {
                                    if let Some(t) = te.changed_touches().get(0) {
                                        let dx = t.client_x() as f64 - swipe_tx_x.get_value();
                                        eid.with_value(|id| {
                                            if dx < -50.0 {
                                                swiped_id.set(Some(id.clone()));
                                            } else if dx > 20.0 && swiped_id.get().as_deref() == Some(id) {
                                                swiped_id.set(None);
                                            }
                                        });
                                    }
                                };

                                let delete_event = move |me: leptos::ev::MouseEvent| {
                                    me.stop_propagation();
                                    let eid_val = eid.get_value();
                                    // Optimistic remove
                                    events.update(|evs| evs.retain(|e| e.id != eid_val));
                                    all_evs.update_value(|v| v.retain(|e| e.id != eid_val));
                                    swiped_id.set(None);
                                    if let Some(db) = get_db() {
                                        let eid_val2 = eid_val.clone();
                                        spawn_local(async move {
                                            delete_event_idb(&db, &eid_val2).await;
                                            if let Some(sig) = current_header.get_untracked() {
                                                let counts = all_evs.with_value(|v| event_row_counts(v, time_boundaries()));
                                                sig.update(|h| {
                                                    h.count_total = counts.3;
                                                    h.count_today = counts.0;
                                                    h.count_week  = counts.1;
                                                    h.count_month = counts.2;
                                                });
                                                save_topic_header(&db, &sig.get_untracked()).await;
                                            }
                                        });
                                    }
                                };

                                let is_swiped = move || {
                                    eid.with_value(|id| swiped_id.get().as_deref() == Some(id))
                                };

                                view! {
                                    <li
                                        class="event-item"
                                        class:swiped=is_swiped
                                        on:touchstart=on_touch_start_row
                                        on:touchend=on_touch_end_row
                                    >
                                        <div class="event-item-content">
                                            <span class="event-icon">"🕐"</span>
                                            <span class="event-time">{format_timestamp(&ts_str)}</span>
                                        </div>
                                        <button
                                            class="btn-delete-swipe"
                                            on:click=delete_event
                                            on:touchend=|te: web_sys::TouchEvent| te.stop_propagation()
                                        >
                                            "Delete"
                                        </button>
                                    </li>
                                }
                            }
                        />
                    </ul>
                    <Show when=has_more>
                        <button class="btn-load-more" on:click=load_more>
                            "Load more"
                        </button>
                    </Show>
                </div>
            </main>

            <Show when=move || show_add_modal.get()>
                <div class="modal-backdrop" on:click=close_add_modal>
                    <div class="modal-sheet" on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()>
                        <p class="modal-title">"Log event"</p>
                        <input
                            type="datetime-local"
                            step="1"
                            class="modal-datetime-input"
                            prop:value=move || manual_dt.get()
                            on:input=move |e| manual_dt.set(event_target_value(&e))
                        />
                        <div class="modal-actions">
                            <button class="modal-btn modal-btn-cancel" on:click=close_add_modal>"Cancel"</button>
                            <button class="modal-btn modal-btn-add" on:click=add_manual_event>"Add"</button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}

#[component]
pub fn App() -> impl IntoView {
    let topic_list: TopicList = RwSignal::new(Vec::new());
    let db_ready_signal = RwSignal::new(false);

    let (new_name, set_new_name) = signal(String::new());
    let editing = RwSignal::new(false);
    let adding = RwSignal::new(false);
    let show_detail: RwSignal<bool> = RwSignal::new(false);
    let detail_id: RwSignal<String> = RwSignal::new(String::new());

    spawn_local(async move {
        let db = open_db().await;
        let headers = load_topic_headers(&db).await;
        DB.with(|cell| *cell.borrow_mut() = Some(db));
        topic_list.set(headers.into_iter().map(RwSignal::new).collect());
        db_ready_signal.set(true);
    });

    provide_context(topic_list);
    provide_context(DbReady(db_ready_signal));
    provide_context(Editing(editing));
    provide_context(ShowDetail(show_detail));
    provide_context(detail_id);

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

            let existing_sig = topic_list.with_untracked(|rows| {
                rows.iter()
                    .find(|s| s.with_untracked(|h| h.name == topic_name))
                    .copied()
            });

            if let Some(sig) = existing_sig {
                let topic_id = sig.with_untracked(|h| h.id.clone());
                spawn_local(async move {
                    let existing = load_events_for_topic(&db, &topic_id).await;
                    let existing_ts: std::collections::HashSet<String> =
                        existing.iter().map(|e| e.timestamp.clone()).collect();
                    let mut all = existing;
                    for mut row in new_rows {
                        if !existing_ts.contains(&row.timestamp) {
                            row.topic_id = topic_id.clone();
                            add_event_idb(&db, &row).await;
                            all.push(row);
                        }
                    }
                    let counts = event_row_counts(&all, time_boundaries());
                    sig.update(|h| {
                        h.count_total = counts.3;
                        h.count_today = counts.0;
                        h.count_week = counts.1;
                        h.count_month = counts.2;
                    });
                    save_topic_header(&db, &sig.get_untracked()).await;
                });
            } else {
                let topic_id = new_id();
                let tid2 = topic_id.clone();
                let name2 = topic_name.clone();
                let rows_clone = new_rows.clone();
                spawn_local(async move {
                    let rows_with_topic: Vec<EventRow> = rows_clone
                        .into_iter()
                        .map(|mut r| {
                            r.topic_id = tid2.clone();
                            r
                        })
                        .collect();
                    let counts = event_row_counts(&rows_with_topic, time_boundaries());
                    let header = TopicHeader {
                        id: tid2.clone(),
                        name: name2,
                        count_total: counts.3,
                        count_today: counts.0,
                        count_week: counts.1,
                        count_month: counts.2,
                    };
                    save_topic_header(&db, &header).await;
                    for row in rows_with_topic {
                        add_event_idb(&db, &row).await;
                    }
                    let header_sig = RwSignal::new(header);
                    topic_list.update(|rows| rows.push(header_sig));
                });
            }
        });

        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        reader.read_as_text(&file).unwrap();
        input.set_value("");
    };

    let add_topic = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let name = new_name.get().trim().to_string();
        if name.is_empty() {
            return;
        }
        let header = TopicHeader {
            id: new_id(),
            name,
            count_total: 0,
            count_today: 0,
            count_week: 0,
            count_month: 0,
        };
        if let Some(db) = get_db() {
            let h2 = header.clone();
            spawn_local(async move {
                save_topic_header(&db, &h2).await;
            });
        }
        topic_list.update(|rows| rows.push(RwSignal::new(header)));
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
                            <label class="header-btn header-btn-import" title="Import from .txt">
                                "↑"
                                <input type="file" accept=".txt" style="display:none" on:change=on_import />
                            </label>
                            <button
                                class="header-btn header-btn-right"
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
                        <Show when=move || !db_ready_signal.get()>
                            <div class="loading-indicator">"Loading…"</div>
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
            >
                <TopicDetail />
            </div>
        </div>
    }
}
