//! The topic detail screen: event list, stats, export/import, manual add.

use crate::app::{DetailEvents, PAGE_SIZE, ShowDetail, ShowEventDetail, TopicList};
use crate::db::{EventRow, delete_event_idb, get_db, load_events_for_topic, save_topic_header};
use crate::logging::{record_event, undo_logged_event};
use crate::stats::StatsCard;
use crate::time::{
    event_row_counts, export_topic, format_timestamp, new_id, now_local_datetime_str,
    time_boundaries,
};
use crate::toasts::Toasts;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

#[component]
pub fn TopicDetail() -> impl IntoView {
    let topic_list = use_context::<TopicList>().expect("topic_list context");
    let show_detail = use_context::<ShowDetail>().expect("show_detail context").0;
    let detail_id = use_context::<RwSignal<String>>().expect("detail_id context");
    let show_event_detail = use_context::<ShowEventDetail>()
        .expect("show_event_detail context")
        .0;
    let event_detail_ev =
        use_context::<RwSignal<Option<EventRow>>>().expect("event_detail_ev context");
    let toasts = use_context::<Toasts>().expect("toasts context");
    let use_detail = use_context::<DetailEvents>().expect("detail_events context");

    let show_add_modal: RwSignal<bool> = RwSignal::new(false);
    let manual_dt: RwSignal<String> = RwSignal::new(String::new());
    let swiped_id: RwSignal<Option<String>> = RwSignal::new(None);

    let DetailEvents {
        page: events,
        all: all_evs,
    } = use_context::<DetailEvents>().expect("detail_events context");
    let loading: RwSignal<bool> = RwSignal::new(false);
    let page_end: RwSignal<usize> = RwSignal::new(PAGE_SIZE);

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
        all_evs.set(Vec::new());
        page_end.set(PAGE_SIZE);
        loading.set(true);
        spawn_local(async move {
            let loaded = load_events_for_topic(&db, &topic_id).await;
            let page = loaded[..PAGE_SIZE.min(loaded.len())].to_vec();
            all_evs.set(loaded);
            events.set(page);
            loading.set(false);
        });
    });

    let load_more = move |_: leptos::ev::MouseEvent| {
        let next = page_end.get() + PAGE_SIZE;
        let slice = all_evs.with_untracked(|v| v[..next.min(v.len())].to_vec());
        events.set(slice);
        page_end.set(next);
    };

    let has_more = move || all_evs.with(|v| page_end.get() < v.len());

    let do_export = move |_: leptos::ev::MouseEvent| {
        let name = topic_name.get_untracked();
        all_evs.with_untracked(|v| export_topic(&name, v));
    };

    let open_add_modal = move |_: leptos::ev::MouseEvent| {
        manual_dt.set(now_local_datetime_str());
        show_add_modal.set(true);
    };
    let close_add_modal = move |_: leptos::ev::MouseEvent| {
        show_add_modal.set(false);
    };

    let add_manual_event = move |_: leptos::ev::MouseEvent| {
        let d = js_sys::Date::new(&JsValue::from_str(&manual_dt.get()));
        if !d.get_time().is_nan()
            && let Some(db) = get_db()
            && let Some(sig) = current_header.get_untracked()
        {
            let row = EventRow {
                id: new_id(),
                topic_id: detail_id.get_untracked(),
                timestamp: d.to_iso_string().as_string().unwrap_or_default(),
                timestamp_ms: d.get_time(),
                ..Default::default()
            };
            spawn_local(async move {
                record_event(
                    &db,
                    row,
                    sig,
                    |row| {
                        use_detail.insert(row);
                        let event_id = row.id.clone();
                        let name = sig.with_untracked(|h| h.name.clone());
                        toasts.show_with_undo(format!("Logged in {name}"), move || {
                            let event_id = event_id.clone();
                            use_detail.remove(&event_id);
                            spawn_local(async move {
                                if let Some(db) = get_db() {
                                    undo_logged_event(&db, &event_id, sig).await;
                                }
                            });
                        });
                    },
                    |row| use_detail.show_enriched(event_detail_ev, row),
                )
                .await;
            });
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
                        <button class="header-btn header-btn-right" type="button" on:click=do_export title="Export to .txt" aria-label="Export topic as text file">"↓"</button>
                        <button class="header-btn header-btn-right" type="button" on:click=open_add_modal title="Log event manually" aria-label="Log event manually">"+"</button>
                    </div>
                </div>
            </header>
            <main class="app-main app-main--detail">
                <Show when=move || all_evs.with(|v| !v.is_empty())>
                    <StatsCard events=all_evs />
                </Show>
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
                            // Timestamp and note are part of the key so an edited row re-renders.
                            key=|ev| format!("{}|{}|{}", ev.id, ev.timestamp_ms, ev.note.as_deref().unwrap_or(""))
                            children=move |ev| {
                                let eid        = StoredValue::new(ev.id.clone());
                                let ts_str     = ev.timestamp.clone();
                                let note       = ev.note.clone();
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
                                    all_evs.update(|v| v.retain(|e| e.id != eid_val));
                                    swiped_id.set(None);
                                    if let Some(db) = get_db() {
                                        let eid_val2 = eid_val.clone();
                                        spawn_local(async move {
                                            delete_event_idb(&db, &eid_val2).await;
                                            if let Some(sig) = current_header.get_untracked() {
                                                let counts = all_evs.with_untracked(|v| event_row_counts(v, time_boundaries()));
                                                sig.update(|h| h.set_counts(counts));
                                                save_topic_header(&db, &sig.get_untracked()).await;
                                            }
                                        });
                                    }
                                };

                                let is_swiped = move || {
                                    eid.with_value(|id| swiped_id.get().as_deref() == Some(id))
                                };

                                // Look the row up at click time: <For> is keyed by id, so a
                                // row later enriched with GPS does not re-run this closure.
                                let open_event_detail = move |me: leptos::ev::MouseEvent| {
                                    me.stop_propagation();
                                    if !is_swiped() {
                                        let current = eid.with_value(|id| {
                                            events.with_untracked(|v| v.iter().find(|e| &e.id == id).cloned())
                                        });
                                        event_detail_ev.set(current);
                                        show_event_detail.set(true);
                                    }
                                };

                                view! {
                                    <li
                                        class="event-item"
                                        class:swiped=is_swiped
                                        on:touchstart=on_touch_start_row
                                        on:touchend=on_touch_end_row
                                    >
                                        <button class="event-item-content" type="button" on:click=open_event_detail>
                                            <span class="event-icon" aria-hidden="true">"🕐"</span>
                                            <span class="event-text">
                                                <span class="event-time">{format_timestamp(&ts_str)}</span>
                                                {note.map(|n| view! { <span class="event-note">{n}</span> })}
                                            </span>
                                        </button>
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
