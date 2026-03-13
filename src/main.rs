use std::cell::Cell;
use std::rc::Rc;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::window;

const STORAGE_KEY: &str = "event_tracker_v1";

// Newtype wrappers so Leptos context lookup never confuses two RwSignal<bool>.
#[derive(Clone, Copy)] struct Editing(RwSignal<bool>);
#[derive(Clone, Copy)] struct ShowDetail(RwSignal<bool>);

// ─── Data model ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TrackedEvent {
    id: String,
    timestamp: String,
    /// Unix epoch in milliseconds (local wall-clock time).
    /// Added later; #[serde(default)] lets old stored records deserialize with 0.0,
    /// which load_topics() then backfills from the ISO timestamp string.
    #[serde(default)]
    timestamp_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Topic {
    id: String,
    name: String,
    events: Vec<TrackedEvent>,
}

// ─── Storage helpers ─────────────────────────────────────────────────────────

fn load_topics() -> Vec<Topic> {
    let storage = window()
        .and_then(|w| w.local_storage().ok())
        .flatten();

    if let Some(s) = storage {
        if let Ok(Some(json)) = s.get_item(STORAGE_KEY) {
            if let Ok(mut topics) = serde_json::from_str::<Vec<Topic>>(&json) {
                // Backfill timestamp_ms for events written before the field existed.
                for topic in &mut topics {
                    for ev in &mut topic.events {
                        if ev.timestamp_ms == 0.0 {
                            ev.timestamp_ms = js_sys::Date::new(
                                &JsValue::from_str(&ev.timestamp)
                            ).get_time();
                        }
                    }
                }
                return topics;
            }
        }
    }
    Vec::new()
}

fn save_topics(topics: &[Topic]) {
    let storage = window()
        .and_then(|w| w.local_storage().ok())
        .flatten();

    if let Some(s) = storage {
        if let Ok(json) = serde_json::to_string(topics) {
            let _ = s.set_item(STORAGE_KEY, &json);
        }
    }
}

// ─── Utilities ────────────────────────────────────────────────────────────────

fn now_timestamp() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}

// Returns the current local date/time as "YYYY-MM-DDTHH:MM:SS" for datetime-local inputs.
fn now_local_datetime_str() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}",
        d.get_full_year(), d.get_month() + 1, d.get_date(),
        d.get_hours(), d.get_minutes(), d.get_seconds(),
    )
}

fn format_timestamp(iso: &str) -> String {
    let d = js_sys::Date::new(&JsValue::from_str(iso));
    format!(
        "{:02}.{:02}.{} - {:02}:{:02}:{:02}",
        d.get_date(),
        d.get_month() + 1,
        d.get_full_year(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds(),
    )
}

fn new_id() -> String {
    let ts = js_sys::Date::now() as u64;
    let rand = (js_sys::Math::random() * 1_000_000.0) as u64;
    format!("{}-{}", ts, rand)
}

// Returns (today, week, month, total) counts for a slice of events.
//
// Performance: computes three ms-based boundaries with 2 Date objects,
// then uses pure arithmetic per event — no per-event Date allocation.
fn event_counts(events: &[TrackedEvent]) -> (usize, usize, usize, usize) {
    let now = js_sys::Date::new_0();
    let now_ms = now.get_time();
    let (cy, cm, cd) = (now.get_full_year(), now.get_month() + 1, now.get_date());

    // Local midnight of today as "YYYY-MM-DDTHH:MM:SS" → Date → ms
    let today_start = js_sys::Date::new(
        &JsValue::from_str(&format!("{}-{:02}-{:02}T00:00:00", cy, cm, cd))
    ).get_time();
    let today_end   = today_start + 86_400_000.0;

    // First ms of the current month
    let month_start = js_sys::Date::new(
        &JsValue::from_str(&format!("{}-{:02}-01T00:00:00", cy, cm))
    ).get_time();

    let week_start = now_ms - 7.0 * 86_400_000.0;

    events.iter().fold((0usize, 0usize, 0usize, events.len()), |(t, w, m, total), ev| {
        let ms = ev.timestamp_ms;
        (
            t + (ms >= today_start && ms < today_end) as usize,
            w + (ms >= week_start && ms <= now_ms)    as usize,
            m + (ms >= month_start)                    as usize,
            total,
        )
    })
}

// Parses one line of the import format "YYYY-MM-DD HH:MM:SS.ffffff"
// into a TrackedEvent with an ISO timestamp (local → UTC).
fn parse_import_line(line: &str) -> Option<TrackedEvent> {
    let line = line.trim();
    if line.is_empty() { return None; }
    let (date_part, time_part) = line.split_once(' ')?;
    // "YYYY-MM-DDTHH:MM:SS.mmm" without 'Z' → parsed as local time by browsers
    let (hms, frac) = time_part.split_once('.').unwrap_or((time_part, "000"));
    let ms_str = format!("{:0<3}", frac);
    let ms = &ms_str[..ms_str.len().min(3)];
    let local_iso = format!("{}T{}.{}", date_part, hms, ms);
    let d = js_sys::Date::new(&JsValue::from_str(&local_iso));
    if d.get_time().is_nan() { return None; }
    let ms = d.get_time();
    Some(TrackedEvent { id: new_id(), timestamp: d.to_iso_string().as_string()?, timestamp_ms: ms })
}

// Formats events as "YYYY-MM-DD HH:MM:SS.000000" (local time) and triggers
// a browser download of "<name>.txt".
fn export_topic(name: &str, events: &[TrackedEvent]) {
    let mut sorted: Vec<&TrackedEvent> = events.iter().collect();
    sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    let content: String = sorted.iter().map(|ev| {
        let d = js_sys::Date::new(&JsValue::from_str(&ev.timestamp));
        format!(
            "{}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}000\n",
            d.get_full_year(), d.get_month() + 1, d.get_date(),
            d.get_hours(), d.get_minutes(), d.get_seconds(),
            d.get_milliseconds() as u32,
        )
    }).collect();

    let arr = js_sys::Array::new();
    arr.push(&JsValue::from_str(&content));
    let blob = web_sys::Blob::new_with_str_sequence(&arr).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

    let doc = window().unwrap().document().unwrap();
    let a: web_sys::HtmlAnchorElement = doc.create_element("a").unwrap()
        .dyn_into().unwrap();
    a.set_href(&url);
    a.set_download(&format!("{}.txt", name));
    doc.body().unwrap().append_child(&a).unwrap();
    a.click();
    doc.body().unwrap().remove_child(&a).unwrap();
    web_sys::Url::revoke_object_url(&url).unwrap();
}

// ─── Components ──────────────────────────────────────────────────────────────

/// Overview row: tapping the main area tracks an event; ‹›› navigates to detail.
#[component]
fn TopicCard(topic_id: String) -> impl IntoView {
    let topics      = use_context::<RwSignal<Vec<Topic>>>().expect("topics context");
    let editing     = use_context::<Editing>().expect("editing context").0;
    let show_detail = use_context::<ShowDetail>().expect("show_detail context").0;
    let detail_id   = use_context::<RwSignal<String>>().expect("detail_id context");

    let tid = StoredValue::new(topic_id);

    let add_event = move |_| {
        topics.update(|ts| {
            tid.with_value(|id| {
                if let Some(t) = ts.iter_mut().find(|t| t.id == *id) {
                    t.events.push(TrackedEvent { id: new_id(), timestamp: now_timestamp(), timestamp_ms: js_sys::Date::now() });
                }
            });
        });
    };

    let delete_topic = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        topics.update(|ts| tid.with_value(|id| ts.retain(|t| t.id != *id)));
    };

    let go_detail = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        tid.with_value(|id| {
            detail_id.set(id.clone());
            show_detail.set(true);
        });
    };

    let topic_name = Memo::new(move |_| {
        tid.with_value(|id| {
            topics.with(|ts| ts.iter().find(|t| t.id == *id).map(|t| t.name.clone()).unwrap_or_default())
        })
    });

    let counts = Memo::new(move |_| {
        tid.with_value(|id| {
            topics.with(|ts| ts.iter().find(|t| t.id == *id)
                .map(|t| event_counts(&t.events))
                .unwrap_or((0, 0, 0, 0)))
        })
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
                        format!("{} today · {} wk · {} mo · {} total",
                            today, week, month, total)
                    }}
                </span>
            </div>
            <button class="btn-detail" on:click=go_detail title="Details">"›"</button>
        </div>
    }
}

/// Full-screen detail view for one topic.
#[component]
fn TopicDetail() -> impl IntoView {
    let topics      = use_context::<RwSignal<Vec<Topic>>>().expect("topics context");
    let show_detail = use_context::<ShowDetail>().expect("show_detail context").0;
    let detail_id   = use_context::<RwSignal<String>>().expect("detail_id context");

    let show_add_modal = RwSignal::new(false);
    let manual_dt      = RwSignal::new(String::new());
    let swiped_id: RwSignal<Option<String>> = RwSignal::new(None);

    let go_back = move |_: leptos::ev::MouseEvent| { show_detail.set(false); };

    let topic_name = Memo::new(move |_| {
        let id = detail_id.get();
        topics.with(|ts| ts.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default())
    });

    let display_events = Memo::new(move |_| {
        let id = detail_id.get();
        let mut evs = topics.with(|ts| {
            ts.iter().find(|t| t.id == id).map(|t| t.events.clone()).unwrap_or_default()
        });
        evs.reverse();
        evs
    });

    let do_export = move |_: leptos::ev::MouseEvent| {
        let id = detail_id.get();
        topics.with(|ts| {
            if let Some(t) = ts.iter().find(|t| t.id == id) {
                export_topic(&t.name, &t.events);
            }
        });
    };

    let open_add_modal = move |_: leptos::ev::MouseEvent| {
        manual_dt.set(now_local_datetime_str());
        show_add_modal.set(true);
    };

    let close_add_modal = move |_: leptos::ev::MouseEvent| { show_add_modal.set(false); };

    let add_manual_event = move |_: leptos::ev::MouseEvent| {
        let dt_str = manual_dt.get();
        let d = js_sys::Date::new(&JsValue::from_str(&dt_str));
        if !d.get_time().is_nan() {
            let iso = d.to_iso_string().as_string().unwrap_or_default();
            let ms  = d.get_time();
            let id = detail_id.get();
            topics.update(|ts| {
                if let Some(t) = ts.iter_mut().find(|t| t.id == id) {
                    t.events.push(TrackedEvent { id: new_id(), timestamp: iso, timestamp_ms: ms });
                }
            });
        }
        show_add_modal.set(false);
    };

    // Swipe-back gesture: start within 40 px of the left edge, drag right ≥ 50 px.
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
                    <button class="header-btn header-btn-back" on:click=go_back>
                        "‹ Back"
                    </button>
                    <h1>{topic_name}</h1>
                    <div class="header-right">
                        <button class="header-btn header-btn-right" on:click=do_export title="Export to .txt">
                            "↓"
                        </button>
                        <button class="header-btn header-btn-right" on:click=open_add_modal title="Log event manually">
                            "+"
                        </button>
                    </div>
                </div>
            </header>
            <main class="app-main app-main--detail">
                <div class="event-card">
                    <ul class="event-list" on:click=move |_| swiped_id.set(None)>
                        <Show
                            when=move || display_events.get().is_empty()
                            fallback=move || view! {
                                <For
                                    each=move || display_events.get()
                                    key=|ev| ev.id.clone()
                                    children=move |ev| {
                                        let eid        = StoredValue::new(ev.id.clone());
                                        let ts_str     = ev.timestamp.clone();
                                        let swipe_tx_x = StoredValue::new(0.0f64);

                                        let on_touch_start = move |te: web_sys::TouchEvent| {
                                            if let Some(t) = te.touches().get(0) {
                                                swipe_tx_x.set_value(t.client_x() as f64);
                                            }
                                        };
                                        let on_touch_end = move |te: web_sys::TouchEvent| {
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
                                            let id = detail_id.get();
                                            topics.update(|ts| {
                                                if let Some(t) = ts.iter_mut().find(|t| t.id == id) {
                                                    eid.with_value(|eid| t.events.retain(|e| e.id != *eid));
                                                }
                                            });
                                            swiped_id.set(None);
                                        };
                                        let is_swiped = move || {
                                            eid.with_value(|id| swiped_id.get().as_deref() == Some(id))
                                        };

                                        view! {
                                            <li
                                                class="event-item"
                                                class:swiped=is_swiped
                                                on:touchstart=on_touch_start
                                                on:touchend=on_touch_end
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
                            }
                        >
                            <li class="event-empty">
                                "No events yet — tap the topic row to log one."
                            </li>
                        </Show>
                    </ul>
                </div>
            </main>

            // ── Manual-event modal ─────────────────────────────────────────────
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
fn App() -> impl IntoView {
    let topics: RwSignal<Vec<Topic>> = RwSignal::new(load_topics());
    let (new_name, set_new_name) = signal(String::new());
    let editing     = RwSignal::new(false);
    let adding      = RwSignal::new(false);
    let show_detail: RwSignal<bool>   = RwSignal::new(false);
    let detail_id:   RwSignal<String> = RwSignal::new(String::new());

    // Debounced save: coalesce rapid updates into a single localStorage write
    // 500 ms after the last change. Uses a JS timeout handle tracked in an Rc<Cell>.
    let save_timer: Rc<Cell<i32>> = Rc::new(Cell::new(-1));
    Effect::new(move |_| {
        let snap = topics.get();
        let win  = window().unwrap();
        // Cancel any pending save
        let prev = save_timer.get();
        if prev >= 0 { win.clear_timeout_with_handle(prev); }
        let timer_clone = save_timer.clone();
        // Closure::once_into_js transfers ownership to JS; no Rust-side drop needed.
        let cb = Closure::once_into_js(move || {
            save_topics(&snap);
            timer_clone.set(-1);
        });
        match win.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.unchecked_ref(), 500
        ) {
            Ok(id) => save_timer.set(id),
            Err(_) => save_topics(&topics.get()), // fallback: save synchronously
        }
    });

    provide_context(topics);
    provide_context(Editing(editing));
    provide_context(ShowDetail(show_detail));
    provide_context(detail_id);

    let on_import = move |ev: leptos::ev::Event| {
        let input: web_sys::HtmlInputElement = ev.target().unwrap().dyn_into().unwrap();
        let files = input.files().unwrap();
        if files.length() == 0 { return; }
        let file = files.get(0).unwrap();

        let filename = file.name();
        let topic_name = filename.strip_suffix(".txt").unwrap_or(&filename).to_string();

        let reader = web_sys::FileReader::new().unwrap();
        let reader_clone = reader.clone();

        let on_load = Closure::once(move |_: JsValue| {
            let text = reader_clone.result().unwrap().as_string().unwrap();
            let new_events: Vec<TrackedEvent> = text.lines()
                .filter_map(parse_import_line)
                .collect();
            topics.update(|ts| {
                if let Some(t) = ts.iter_mut().find(|t| t.name == topic_name) {
                    for ev in new_events {
                        if !t.events.iter().any(|e| e.timestamp == ev.timestamp) {
                            t.events.push(ev);
                        }
                    }
                } else {
                    ts.push(Topic { id: new_id(), name: topic_name, events: new_events });
                }
            });
        });

        reader.set_onload(Some(on_load.as_ref().unchecked_ref()));
        on_load.forget();
        reader.read_as_text(&file).unwrap();
        input.set_value("");
    };

    let add_topic = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let name = new_name.get().trim().to_string();
        if !name.is_empty() {
            topics.update(|ts| {
                ts.push(Topic { id: new_id(), name, events: Vec::new() });
            });
            set_new_name.set(String::new());
            adding.set(false);
        }
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
                        <button
                            class="header-btn header-btn-left"
                            on:click=move |_| editing.update(|e| *e = !*e)
                        >
                            {move || if editing.get() { "Done" } else { "Edit" }}
                        </button>
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
                        <Show
                            when=move || topics.get().is_empty()
                            fallback=move || view! {
                                <For
                                    each=move || topics.get()
                                    key=|t| t.id.clone()
                                    children=|t| view! { <TopicCard topic_id=t.id /> }
                                />
                            }
                        >
                            <div class="empty-state">
                                <p>"No topics yet."</p>
                                <p>"Tap \"+\" to add one."</p>
                            </div>
                        </Show>
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

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(|| view! { <App /> });
}
