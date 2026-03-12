use leptos::prelude::*;
use serde::{Deserialize, Serialize};
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

// ─── Components ──────────────────────────────────────────────────────────────

#[component]
fn TopicCard(topic_id: String) -> impl IntoView {
    let topics = use_context::<RwSignal<Vec<Topic>>>().expect("topics context");
    let editing = use_context::<RwSignal<bool>>().expect("editing context");
    let (expanded, set_expanded) = signal(false);

    // StoredValue is Copy, so a single tid can be captured by every closure,
    // including those nested inside <Show>/<For> children that require Fn.
    let tid = StoredValue::new(topic_id);

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
                                            <span class="event-time">{ev.timestamp}</span>
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
