use leptos::prelude::*;
use serde::{Deserialize, Serialize};
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

// ─── Components ──────────────────────────────────────────────────────────────

#[component]
fn TopicCard(topic_id: String) -> impl IntoView {
    let topics = use_context::<RwSignal<Vec<Topic>>>().expect("topics context");
    let (expanded, set_expanded) = signal(false);

    // Separate clones so each closure owns its ID.
    let id1 = topic_id.clone();
    let add_event = move |_| {
        topics.update(|ts| {
            if let Some(t) = ts.iter_mut().find(|t| t.id == id1) {
                t.events.push(TrackedEvent { id: new_id(), timestamp: now_timestamp() });
            }
        });
    };

    let id2 = topic_id.clone();
    let delete_topic = move |_| {
        topics.update(|ts| ts.retain(|t| t.id != id2));
    };

    // Use Memo (Copy) so these signals can be captured inside <Show> children
    // closures that must implement Fn (called each time the Show condition changes).
    let id3 = topic_id.clone();
    let topic_name = Memo::new(move |_| {
        topics.with(|ts| ts.iter().find(|t| t.id == id3).map(|t| t.name.clone()).unwrap_or_default())
    });

    let id4 = topic_id.clone();
    let event_count = Memo::new(move |_| {
        topics.with(|ts| ts.iter().find(|t| t.id == id4).map(|t| t.events.len()).unwrap_or(0))
    });

    let id5 = topic_id;
    let display_events = Memo::new(move |_| {
        let mut evs = topics.with(|ts| ts.iter().find(|t| t.id == id5).map(|t| t.events.clone()).unwrap_or_default());
        evs.reverse();
        evs
    });

    view! {
        <div class="topic-card">
            <div class="topic-header">
                <div class="topic-title">
                    <h2>{topic_name}</h2>
                    <span class="event-count">
                        {move || {
                            let count = event_count.get();
                            if count == 1 { "1 event".to_string() } else { format!("{} events", count) }
                        }}
                    </span>
                </div>
                <div class="topic-actions">
                    <button class="btn btn-primary" on:click=add_event>"+ Add"</button>
                    <button
                        class="btn btn-ghost"
                        on:click=move |_| set_expanded.update(|e| *e = !*e)
                    >
                        {move || if expanded.get() { "▲ Hide" } else { "▼ Show" }}
                    </button>
                    <button class="btn btn-danger" on:click=delete_topic title="Delete topic">
                        "✕"
                    </button>
                </div>
            </div>
            <Show when=move || expanded.get()>
                <ul class="event-list">
                    <Show
                        when=move || event_count.get() == 0
                        fallback=move || view! {
                            <For
                                each=move || display_events.get()
                                key=|ev| ev.id.clone()
                                children=|ev| view! {
                                    <li class="event-item">
                                        <span class="event-icon">"🕐"</span>
                                        {ev.timestamp}
                                    </li>
                                }
                            />
                        }
                    >
                        <li class="event-empty">
                            "No events yet — press \"+ Add\" to log one."
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

    // Persist to localStorage whenever topics change
    Effect::new(move |_| {
        save_topics(&topics.get());
    });

    provide_context(topics);

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
        }
    };

    view! {
        <div class="app">
            <header class="app-header">
                <h1>"trackit"</h1>
            </header>

            <main class="app-main">
                // ── Add topic form ──────────────────────────────────────
                <form class="add-topic-form" on:submit=add_topic>
                    <input
                        class="topic-input"
                        type="text"
                        placeholder="New topic (e.g. standing up, water…)"
                        prop:value=new_name
                        on:input=move |e| set_new_name.set(event_target_value(&e))
                    />
                    <button class="btn btn-add" type="submit">"Add Topic"</button>
                </form>

                // ── Topic list ──────────────────────────────────────────
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
                            <p>"Create one above to start tracking!"</p>
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
