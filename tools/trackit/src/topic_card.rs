//! One row of the topic list (log, rename, reorder, delete).

use crate::app::{DetailEvents, Editing, PendingDelete, ShowDetail, TopicList};
use crate::db::{
    EventRow, TopicHeader, delete_topic_idb, get_db, save_topic_header, save_topic_headers_idb,
};
use crate::import::other_topic_names;
use crate::logging::{record_event, undo_logged_event};
use crate::time::{Direction, move_item, new_id, now_timestamp, validate_topic_name};
use crate::toasts::Toasts;
use leptos::prelude::*;
use leptos::task::spawn_local;

#[component]
pub fn TopicCard(topic_signal: RwSignal<TopicHeader>) -> impl IntoView {
    let topic_list = use_context::<TopicList>().expect("topic_list context");
    let editing = use_context::<Editing>().expect("editing context").0;
    let show_detail = use_context::<ShowDetail>().expect("show_detail context").0;
    let detail_id = use_context::<RwSignal<String>>().expect("detail_id context");
    let toasts = use_context::<Toasts>().expect("toasts context");
    let detail_events = use_context::<DetailEvents>().expect("detail_events context");
    let event_detail_ev =
        use_context::<RwSignal<Option<EventRow>>>().expect("event_detail_ev context");
    let pending_delete = use_context::<PendingDelete>()
        .expect("pending_delete context")
        .0;
    let is_pending_delete =
        move || pending_delete.with(|p| p.as_deref() == Some(&topic_signal.with(|h| h.id.clone())));

    // Draft name while renaming inline (edit mode only).
    let renaming: RwSignal<Option<String>> = RwSignal::new(None);
    let rename_input = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(input) = rename_input.get() {
            let _ = input.focus();
            input.select();
        }
    });
    let commit_rename = move || {
        let Some(draft) = renaming.get_untracked() else {
            return;
        };
        renaming.set(None);
        let (id, current) = topic_signal.with_untracked(|h| (h.id.clone(), h.name.clone()));
        match validate_topic_name(&draft, &other_topic_names(topic_list, &id)) {
            Ok(name) if name == current => {}
            Ok(name) => {
                topic_signal.update(|h| h.name = name.clone());
                if let Some(db) = get_db() {
                    let header = topic_signal.get_untracked();
                    spawn_local(async move {
                        save_topic_header(&db, &header).await;
                    });
                }
                toasts.show(format!("Renamed to “{name}” – update ?add= shortcuts"));
            }
            Err(e) => toasts.show(e.message()),
        }
    };

    let add_event = move |_| {
        if editing.get_untracked() {
            // Edit mode never logs: a tap cancels a pending delete or renames.
            if pending_delete.with_untracked(Option::is_some) {
                pending_delete.set(None);
            } else {
                renaming.set(Some(topic_signal.with_untracked(|h| h.name.clone())));
            }
            return;
        }
        let Some(db) = get_db() else { return };
        let row = EventRow {
            id: new_id(),
            topic_id: topic_signal.with_untracked(|h| h.id.clone()),
            timestamp: now_timestamp(),
            timestamp_ms: js_sys::Date::now(),
            ..Default::default()
        };
        spawn_local(async move {
            record_event(
                &db,
                row,
                topic_signal,
                |row| {
                    let event_id = row.id.clone();
                    let name = topic_signal.with_untracked(|h| h.name.clone());
                    toasts.show_with_undo(format!("Logged in {name}"), move || {
                        let event_id = event_id.clone();
                        spawn_local(async move {
                            if let Some(db) = get_db() {
                                undo_logged_event(&db, &event_id, topic_signal).await;
                            }
                        });
                    });
                },
                |row| detail_events.show_enriched(event_detail_ev, row),
            )
            .await;
        });
    };

    // "−" only arms the delete; the revealed "Delete" button performs it.
    let arm_delete = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        let id = topic_signal.with_untracked(|h| h.id.clone());
        pending_delete.update(|p| {
            *p = if p.as_deref() == Some(&id) {
                None
            } else {
                Some(id)
            }
        });
    };

    let delete_topic = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        pending_delete.set(None);
        let id = topic_signal.with_untracked(|h| h.id.clone());
        let id2 = id.clone();
        if let Some(db) = get_db() {
            spawn_local(async move {
                delete_topic_idb(&db, &id).await;
            });
        }
        topic_list.update(|rows| rows.retain(|s| s.with_untracked(|h| h.id != id2)));
    };

    // ▲/▼ in edit mode: swap with the neighbour, then persist every position
    // so legacy records (all position 0) get a stable order too.
    let move_topic = move |dir: Direction| {
        let id = topic_signal.with_untracked(|h| h.id.clone());
        let mut moved = false;
        topic_list.update(|rows| {
            if let Some(idx) = rows.iter().position(|s| s.with_untracked(|h| h.id == id)) {
                moved = move_item(rows, idx, dir);
            }
        });
        if !moved {
            return;
        }
        let headers: Vec<TopicHeader> = topic_list.with_untracked(|rows| {
            rows.iter()
                .enumerate()
                .map(|(i, s)| {
                    let i = i as u32;
                    if s.with_untracked(|h| h.position != i) {
                        s.update(|h| h.position = i);
                    }
                    s.get_untracked()
                })
                .collect()
        });
        if let Some(db) = get_db() {
            spawn_local(async move {
                save_topic_headers_idb(&db, &headers).await;
            });
        }
    };
    let is_at = move |first: bool| {
        let id = topic_signal.with(|h| h.id.clone());
        topic_list.with(|rows| {
            let end = if first { rows.first() } else { rows.last() };
            end.is_some_and(|s| s.with_untracked(|h| h.id == id))
        })
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
                <button
                    class="btn-delete-topic"
                    type="button"
                    on:click=arm_delete
                    title="Delete topic"
                    aria-label=move || format!("Delete topic {}", topic_name.get())
                >
                    "−"
                </button>
            </Show>
            <Show
                when=move || editing.get() && renaming.with(Option::is_some)
                fallback=move || view! {
                    <button
                        class="topic-row-main"
                        type="button"
                        on:click=add_event
                        aria-label=move || {
                            let verb = if editing.get() { "Rename" } else { "Log event for" };
                            format!("{verb} {}", topic_name.get())
                        }
                    >
                        <span class="topic-row-name">{topic_name}</span>
                        <span class="topic-row-counts">
                            {move || {
                                let (today, week, month, total) = counts.get();
                                format!("{} today · {} wk · {} mo · {} total", today, week, month, total)
                            }}
                        </span>
                    </button>
                }
            >
                <input
                    class="topic-input topic-rename-input"
                    type="text"
                    node_ref=rename_input
                    aria-label="Topic name"
                    prop:value=move || renaming.get().unwrap_or_default()
                    on:input=move |e| renaming.set(Some(event_target_value(&e)))
                    on:keydown=move |e: leptos::ev::KeyboardEvent| match e.key().as_str() {
                        "Enter" => commit_rename(),
                        "Escape" => renaming.set(None),
                        _ => {}
                    }
                    on:blur=move |_| commit_rename()
                />
            </Show>
            <Show
                when=is_pending_delete
                fallback=move || if editing.get() {
                    view! {
                        <div class="reorder-buttons">
                            <button
                                class="btn-reorder"
                                type="button"
                                disabled=move || is_at(true)
                                on:click=move |_| move_topic(Direction::Up)
                                aria-label=move || format!("Move {} up", topic_name.get())
                            >
                                "▲"
                            </button>
                            <button
                                class="btn-reorder"
                                type="button"
                                disabled=move || is_at(false)
                                on:click=move |_| move_topic(Direction::Down)
                                aria-label=move || format!("Move {} down", topic_name.get())
                            >
                                "▼"
                            </button>
                        </div>
                    }
                    .into_any()
                } else {
                    view! {
                        <button
                            class="btn-detail"
                            type="button"
                            on:click=go_detail
                            title="Details"
                            aria-label=move || format!("Details for {}", topic_name.get())
                        >
                            "›"
                        </button>
                    }
                    .into_any()
                }
            >
                <button
                    class="btn-confirm-delete"
                    type="button"
                    on:click=delete_topic
                    aria-label=move || format!("Confirm deleting {}", topic_name.get())
                >
                    "Delete"
                </button>
            </Show>
        </div>
    }
}
