use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::window;

const STORAGE_KEY: &str = "event_tracker_v1";

// ─── Data model ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TrackedEvent {
    id: String,
    timestamp: String,
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
            if let Ok(topics) = serde_json::from_str::<Vec<Topic>>(&json) {
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
fn event_counts(events: &[TrackedEvent]) -> (usize, usize, usize, usize) {
    let now = js_sys::Date::new_0();
    let now_ms = js_sys::Date::now();
    let (cy, cm, cd) = (now.get_full_year(), now.get_month(), now.get_date());
    let week_ms = 7.0 * 24.0 * 3600.0 * 1000.0;

    let mut today = 0usize;
    let mut week  = 0usize;
    let mut month = 0usize;

    for ev in events {
        let d = js_sys::Date::new(&JsValue::from_str(&ev.timestamp));
        let ev_ms = d.get_time();
        if d.get_full_year() == cy && d.get_month() == cm && d.get_date() == cd {
            today += 1;
        }
        if now_ms - ev_ms < week_ms {
            week += 1;
        }
        if d.get_full_year() == cy && d.get_month() == cm {
            month += 1;
        }
    }
    (today, week, month, events.len())
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
    Some(TrackedEvent { id: new_id(), timestamp: d.to_iso_string().as_string()? })
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

#[component]
fn TopicCard(topic_id: String) -> impl IntoView {
    let topics = use_context::<RwSignal<Vec<Topic>>>().expect("topics context");
    let editing = use_context::<RwSignal<bool>>().expect("editing context");
    let (expanded, set_expanded) = signal(false);

    // StoredValue is Copy, so a single tid can be captured by every closure,
    // including those nested inside <Show>/<For> children that require Fn.
    let tid = StoredValue::new(topic_id);

    let do_export = move |_| {
        tid.with_value(|id| topics.with(|ts| {
            if let Some(t) = ts.iter().find(|t| t.id == *id) {
                export_topic(&t.name, &t.events);
            }
        }));
    };

    let add_event = move |_| {
        topics.update(|ts| {
            tid.with_value(|id| {
                if let Some(t) = ts.iter_mut().find(|t| t.id == *id) {
                    t.events.push(TrackedEvent { id: new_id(), timestamp: now_timestamp() });
                }
            });
        });
    };

    let delete_topic = move |_| {
        topics.update(|ts| tid.with_value(|id| ts.retain(|t| t.id != *id)));
    };

    let topic_name = Memo::new(move |_| {
        tid.with_value(|id| topics.with(|ts| ts.iter().find(|t| t.id == *id).map(|t| t.name.clone()).unwrap_or_default()))
    });

    // (today, week, month, total)
    let counts = Memo::new(move |_| {
        tid.with_value(|id| topics.with(|ts| {
            ts.iter().find(|t| t.id == *id)
                .map(|t| event_counts(&t.events))
                .unwrap_or((0, 0, 0, 0))
        }))
    });

    let display_events = Memo::new(move |_| {
        let mut evs = tid.with_value(|id| topics.with(|ts| ts.iter().find(|t| t.id == *id).map(|t| t.events.clone()).unwrap_or_default()));
        evs.reverse();
        evs
    });

    view! {
        <div class="topic-card">
            <div class="topic-header">
                <Show when=move || editing.get()>
                    <button class="btn-delete-topic" on:click=delete_topic title="Delete topic">
                        "−"
                    </button>
                </Show>
                <div
                    class="topic-title"
                    on:click=move |_| set_expanded.update(|e| *e = !*e)
                >
                    <h2>{topic_name}</h2>
                    <span class="event-count">
                        {move || {
                            let (today, week, month, total) = counts.get();
                            format!("{} today · {} wk · {} mo · {} total",
                                today, week, month, total)
                        }}
                    </span>
                </div>
                <div class="topic-actions">
                    <button class="btn-export" on:click=do_export title="Export to .txt">"↓"</button>
                    <button class="btn-add-event" on:click=add_event title="Add event">"+"</button>
                    <button
                        class="btn-chevron"
                        on:click=move |_| set_expanded.update(|e| *e = !*e)
                    >
                        {move || if expanded.get() { "▲" } else { "▼" }}
                    </button>
                </div>
            </div>
            <Show when=move || expanded.get()>
                <ul class="event-list">
                    <Show
                        when=move || counts.get().3 == 0
                        fallback=move || view! {
                            <For
                                each=move || display_events.get()
                                key=|ev| ev.id.clone()
                                children=move |ev| {
                                    // StoredValue is Copy so delete_event can be Fn
                                    let eid = StoredValue::new(ev.id);
                                    let delete_event = move |_| {
                                        topics.update(|ts| {
                                            tid.with_value(|id| {
                                                if let Some(t) = ts.iter_mut().find(|t| t.id == *id) {
                                                    eid.with_value(|eid| t.events.retain(|e| e.id != *eid));
                                                }
                                            });
                                        });
                                    };
                                    view! {
                                        <li class="event-item">
                                            <span class="event-icon">"🕐"</span>
                                            <span class="event-time">{format_timestamp(&ev.timestamp)}</span>
                                            <button class="btn-delete-event" on:click=delete_event title="Delete">"✕"</button>
                                        </li>
                                    }
                                }
                            />
                        }
                    >
                        <li class="event-empty">
                            "No events yet — press \"+\" to log one."
                        </li>
                    </Show>
                </ul>
            </Show>
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let topics: RwSignal<Vec<Topic>> = RwSignal::new(load_topics());
    let (new_name, set_new_name) = signal(String::new());
    let editing = RwSignal::new(false);
    let adding = RwSignal::new(false);

    // Persist to localStorage whenever topics change
    Effect::new(move |_| {
        save_topics(&topics.get());
    });

    provide_context(topics);
    provide_context(editing);

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
                ts.push(Topic {
                    id: new_id(),
                    name,
                    events: Vec::new(),
                });
            });
            set_new_name.set(String::new());
            adding.set(false);
        }
    };

    view! {
        <div class="app">
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
                                if !now_adding {
                                    set_new_name.set(String::new());
                                }
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
                                children=|topic| view! { <TopicCard topic_id=topic.id /> }
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
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(|| view! { <App /> });
}
