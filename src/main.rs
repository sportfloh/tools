use leptos::*;
use serde::{Deserialize, Serialize};
use web_sys::window;

const STORAGE_KEY: &str = "event_tracker_v1";

// ─── Data model ──────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize, PartialEq)]
struct TrackedEvent {
    id: String,
    timestamp: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
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

// ─── Utility ─────────────────────────────────────────────────────────────────

fn now_timestamp() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds()
    )
}

fn new_id() -> String {
    format!("{}", js_sys::Date::now() as u64)
}

// ─── Components ──────────────────────────────────────────────────────────────

#[component]
fn TopicCard(topic_id: String, topics: RwSignal<Vec<Topic>>) -> impl IntoView {
    let (expanded, set_expanded) = create_signal(false);

    let tid_for_derive = topic_id.clone();
    let topic = Signal::derive(move || {
        topics
            .get()
            .into_iter()
            .find(|t| t.id == tid_for_derive)
            .unwrap_or_else(|| Topic {
                id: String::new(),
                name: String::new(),
                events: Vec::new(),
            })
    });

    let tid_for_add = topic_id.clone();
    let add_event = move |_| {
        let new_event = TrackedEvent {
            id: new_id(),
            timestamp: now_timestamp(),
        };
        topics.update(|ts| {
            if let Some(t) = ts.iter_mut().find(|t| t.id == tid_for_add) {
                t.events.push(new_event);
            }
        });
    };

    let tid_for_delete = topic_id.clone();
    let delete_topic = move |_| {
        topics.update(|ts| ts.retain(|t| t.id != tid_for_delete));
    };

    view! {
        <div class="topic-card">
            <div class="topic-header">
                <div class="topic-title">
                    <h2>{move || topic.get().name}</h2>
                    <span class="event-count">
                        {move || {
                            let count = topic.get().events.len();
                            if count == 1 { "1 event".to_string() } else { format!("{} events", count) }
                        }}
                    </span>
                </div>
                <div class="topic-actions">
                    <button class="btn btn-primary" on:click=add_event>
                        "+ Add"
                    </button>
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
                    {move || {
                        let mut events = topic.get().events;
                        events.reverse();
                        if events.is_empty() {
                            view! {
                                <li class="event-empty">"No events yet — press \"+ Add\" to log one."</li>
                            }.into_view()
                        } else {
                            events
                                .into_iter()
                                .map(|event| {
                                    view! {
                                        <li class="event-item">
                                            <span class="event-icon">"🕐"</span>
                                            {event.timestamp}
                                        </li>
                                    }
                                })
                                .collect_view()
                        }
                    }}
                </ul>
            </Show>
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let topics = create_rw_signal(load_topics());
    let (new_name, set_new_name) = create_signal(String::new());

    // Persist to localStorage whenever topics change
    create_effect(move |_| {
        save_topics(&topics.get());
    });

    let add_topic = move |_| {
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

    let handle_keydown = move |e: web_sys::KeyboardEvent| {
        if e.key() == "Enter" {
            add_topic(web_sys::MouseEvent::new("click").unwrap());
        }
    };

    view! {
        <div class="app">
            <header class="app-header">
                <h1>"📋 Event Tracker"</h1>
                <p class="subtitle">"Track anything, one tap at a time."</p>
            </header>

            <main class="app-main">
                // ── Add topic form ──────────────────────────────────────────
                <div class="add-topic-form">
                    <input
                        class="topic-input"
                        type="text"
                        placeholder="New topic (e.g. standing up, water…)"
                        prop:value=new_name
                        on:input=move |e| set_new_name.set(event_target_value(&e))
                        on:keydown=handle_keydown
                    />
                    <button class="btn btn-add" on:click=add_topic>
                        "Add Topic"
                    </button>
                </div>

                // ── Topic cards ─────────────────────────────────────────────
                <div class="topic-list">
                    <Show
                        when=move || topics.get().is_empty()
                        fallback=move || view! {
                            <For
                                each=move || topics.get()
                                key=|t| t.id.clone()
                                children=move |topic| {
                                    let id = topic.id.clone();
                                    view! { <TopicCard topic_id=id topics=topics /> }
                                }
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
    mount_to_body(App);
}
