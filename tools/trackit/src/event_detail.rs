//! The event detail screen: time, note and location of one event.

use crate::app::{DetailEvents, ShowEventDetail, TopicList};
use crate::db::{EventRow, get_db, refresh_topic_counts_idb, update_event_idb};
use crate::time::{local_datetime_str, map_url};
use crate::toasts::Toasts;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

#[component]
pub fn EventDetail() -> impl IntoView {
    let show_event_detail = use_context::<ShowEventDetail>()
        .expect("show_event_detail context")
        .0;
    let event_detail_ev =
        use_context::<RwSignal<Option<EventRow>>>().expect("event_detail_ev context");

    let go_back = move |_: leptos::ev::MouseEvent| {
        show_event_detail.set(false);
    };

    // Swipe right from left edge to go back
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
                show_event_detail.set(false);
            }
        }
    };

    let fmt_opt = |v: Option<f64>, unit: &'static str, decimals: usize| {
        v.map(|n| format!("{:.prec$} {}", n, unit, prec = decimals))
            .unwrap_or_else(|| "—".into())
    };

    let ev = move || event_detail_ev.get();

    // ── Editable note and time ────────────────────────────────────────────
    let detail = use_context::<DetailEvents>().expect("detail_events context");
    let topic_list = use_context::<TopicList>().expect("topic_list context");
    let toasts = use_context::<Toasts>().expect("toasts context");
    let note_draft = RwSignal::new(String::new());
    let time_draft = RwSignal::new(String::new());
    // Reset the drafts only when a different event is opened, so a GPS fix
    // arriving while typing does not wipe the draft.
    let open_id = Memo::new(move |_| event_detail_ev.with(|e| e.as_ref().map(|e| e.id.clone())));
    Effect::new(move |_| {
        open_id.track();
        if let Some(e) = event_detail_ev.get_untracked() {
            note_draft.set(e.note.clone().unwrap_or_default());
            time_draft.set(local_datetime_str(e.timestamp_ms));
        }
    });
    let save_row = move |row: EventRow, done: &'static str, counts_changed: bool| {
        let Some(db) = get_db() else { return };
        spawn_local(async move {
            if !update_event_idb(&db, &row).await {
                toasts.show("Not saved – the event no longer exists");
                return;
            }
            detail.replace(&row);
            event_detail_ev.set(Some(row.clone()));
            if counts_changed {
                let sig = topic_list.with_untracked(|rows| {
                    rows.iter()
                        .find(|s| s.with_untracked(|h| h.id == row.topic_id))
                        .copied()
                });
                if let Some(sig) = sig {
                    let fresh = refresh_topic_counts_idb(&db, &sig.get_untracked()).await;
                    sig.set(fresh);
                }
            }
            toasts.show(done);
        });
    };
    let save_note = move |_| {
        let Some(current) = event_detail_ev.get_untracked() else {
            return;
        };
        let draft = note_draft.get_untracked();
        let note = Some(draft.trim().to_string()).filter(|n| !n.is_empty());
        if note != current.note {
            save_row(EventRow { note, ..current }, "Note saved", false);
        }
    };
    // Compare instants, not strings: browsers drop ":00" seconds from the value.
    let time_changed = move || {
        let draft_ms = js_sys::Date::new(&JsValue::from_str(&time_draft.get())).get_time();
        event_detail_ev.with(|e| {
            e.as_ref().is_some_and(|e| {
                draft_ms.is_nan()
                    || (draft_ms - (e.timestamp_ms / 1000.0).floor() * 1000.0).abs() >= 1000.0
            })
        })
    };
    let save_time = move |_| {
        let Some(current) = event_detail_ev.get_untracked() else {
            return;
        };
        let d = js_sys::Date::new(&JsValue::from_str(&time_draft.get_untracked()));
        if d.get_time().is_nan() {
            toasts.show("Invalid date");
            return;
        }
        let row = EventRow {
            timestamp: d.to_iso_string().as_string().unwrap_or_default(),
            timestamp_ms: d.get_time(),
            ..current
        };
        save_row(row, "Time updated", true);
    };

    view! {
        <div
            class="event-detail-wrapper"
            on:touchstart=on_touch_start
            on:touchend=on_touch_end
        >
            <header class="app-header">
                <div class="header-bar">
                    <button class="header-btn header-btn-back" on:click=go_back>"‹ Back"</button>
                    <h1>"Event"</h1>
                    <div class="header-btn"></div>
                </div>
            </header>
            <div class="event-detail-main">
                // Rendered once per opened event (not on every update), so
                // the fields keep focus while a save or GPS fix lands.
                <Show when=move || open_id.with(Option::is_some)>
                    <div class="event-detail-card event-edit-card">
                        <div class="event-detail-section">
                            <label class="event-detail-row event-edit-row">
                                <span class="event-detail-label">"Time"</span>
                                <input
                                    class="event-time-input"
                                    type="datetime-local"
                                    step="1"
                                    prop:value=time_draft
                                    on:input=move |e| time_draft.set(event_target_value(&e))
                                />
                            </label>
                            <Show when=time_changed>
                                <div class="event-edit-actions">
                                    <button class="event-save-btn" type="button" on:click=save_time>
                                        "Save time"
                                    </button>
                                </div>
                            </Show>
                        </div>
                        <div class="event-detail-section">
                            <label class="event-note-field">
                                <span class="event-detail-label">"Note"</span>
                                <textarea
                                    class="event-note-input"
                                    rows="3"
                                    placeholder="Add a note…"
                                    prop:value=note_draft
                                    on:input=move |e| note_draft.set(event_target_value(&e))
                                    on:blur=save_note
                                />
                            </label>
                        </div>
                    </div>
                </Show>
                <Show when=move || ev().is_some()>
                    {move || ev().map(|e| {
                        let lat_str  = e.lat.map(|v| format!("{:.6}°", v)).unwrap_or("—".into());
                        let lon_str  = e.lon.map(|v| format!("{:.6}°", v)).unwrap_or("—".into());
                        let alt_str  = fmt_opt(e.altitude, "m", 1);
                        let hdg_str  = fmt_opt(e.heading, "°", 1);
                        let spd_str  = e.speed.map(|v| format!("{:.1} m/s", v)).unwrap_or("—".into());
                        let acc_str  = fmt_opt(e.accuracy, "m ±", 1);
                        let aac_str  = fmt_opt(e.altitude_accuracy, "m ±", 1);
                        let map_href = e.lat.zip(e.lon).map(|(lat, lon)| map_url(lat, lon));
                        view! {
                            <div class="event-detail-card">
                                <div class="event-detail-section">
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Latitude"</span>
                                        <span class="event-detail-value">{lat_str}</span>
                                    </div>
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Longitude"</span>
                                        <span class="event-detail-value">{lon_str}</span>
                                    </div>
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Altitude"</span>
                                        <span class="event-detail-value">{alt_str}</span>
                                    </div>
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Accuracy"</span>
                                        <span class="event-detail-value">{acc_str}</span>
                                    </div>
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Alt. accuracy"</span>
                                        <span class="event-detail-value">{aac_str}</span>
                                    </div>
                                    {map_href.map(|href| view! {
                                        <div class="event-detail-row">
                                            <a
                                                class="event-map-link"
                                                href=href
                                                target="_blank"
                                                rel="noopener"
                                            >
                                                "Show on map ↗"
                                            </a>
                                        </div>
                                    })}
                                </div>
                                <div class="event-detail-section">
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Heading"</span>
                                        <span class="event-detail-value">{hdg_str}</span>
                                    </div>
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Speed"</span>
                                        <span class="event-detail-value">{spd_str}</span>
                                    </div>
                                </div>
                            </div>
                        }
                    })}
                </Show>
            </div>
        </div>
    }
}
