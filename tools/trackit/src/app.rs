use crate::db::{
    DB, EventRow, TopicHeader, add_event_and_update_header_idb, add_events_bulk_idb,
    delete_event_idb, delete_topic_idb, enrich_event_idb, get_db, load_events_for_topic,
    load_topic_headers, open_db, refresh_topic_counts_idb, save_topic_header,
};
use crate::time::{
    event_row_counts, export_all, export_topic, format_timestamp, merge_new_events, new_id,
    now_local_datetime_str, now_timestamp, parse_bulk_import, parse_import_line, time_boundaries,
    with_added_event,
};
use leptos::prelude::*;
use leptos::task::spawn_local;
use rexie::Rexie;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

// ─── GPS snapshot ─────────────────────────────────────────────────────────────

struct GpsSnapshot {
    lat: f64,
    lon: f64,
    altitude: Option<f64>,
    heading: Option<f64>,
    speed: Option<f64>,
    accuracy: f64,
    altitude_accuracy: Option<f64>,
}

async fn get_gps() -> Option<GpsSnapshot> {
    let window = web_sys::window()?;
    let geo = window.navigator().geolocation().ok()?;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let options = web_sys::PositionOptions::new();
        options.set_enable_high_accuracy(true);
        options.set_timeout(10_000);
        let on_success = Closure::once(move |pos: JsValue| {
            let _ = resolve.call1(&JsValue::NULL, &pos);
        });
        let on_error = Closure::once(move |_: JsValue| {
            let _ = reject.call0(&JsValue::NULL);
        });
        let _ = geo.get_current_position_with_error_callback_and_options(
            on_success.as_ref().unchecked_ref(),
            Some(on_error.as_ref().unchecked_ref()),
            &options,
        );
        on_success.forget();
        on_error.forget();
    });
    let pos_val = JsFuture::from(promise).await.ok()?;
    // Extract via JS reflection — avoids depending on GeolocationPosition feature gating.
    let coords = js_sys::Reflect::get(&pos_val, &JsValue::from_str("coords")).ok()?;
    let get = |key: &str| js_sys::Reflect::get(&coords, &JsValue::from_str(key)).ok();
    let lat = get("latitude")?.as_f64()?;
    let lon = get("longitude")?.as_f64()?;
    let accuracy = get("accuracy")?.as_f64()?;
    let altitude = get("altitude").and_then(|v| v.as_f64());
    let altitude_accuracy = get("altitudeAccuracy").and_then(|v| v.as_f64());
    let heading = get("heading").and_then(|v| v.as_f64());
    let speed = get("speed").and_then(|v| v.as_f64());
    Some(GpsSnapshot {
        lat,
        lon,
        altitude,
        heading,
        speed,
        accuracy,
        altitude_accuracy,
    })
}

impl GpsSnapshot {
    fn applied_to(self, row: EventRow) -> EventRow {
        EventRow {
            lat: Some(self.lat),
            lon: Some(self.lon),
            altitude: self.altitude,
            heading: self.heading,
            speed: self.speed,
            accuracy: Some(self.accuracy),
            altitude_accuracy: self.altitude_accuracy,
            ..row
        }
    }
}

/// Log `row` for the topic in `header`: persist the event together with the
/// bumped counts, then attach a GPS fix in the background once one arrives.
/// `on_saved` runs after the event has been persisted, `on_enriched` after the
/// GPS fix has been written. The fix is dropped if the event was deleted while
/// waiting for it.
async fn record_event(
    db: &Rexie,
    row: EventRow,
    header: RwSignal<TopicHeader>,
    on_saved: impl FnOnce(&EventRow),
    on_enriched: impl FnOnce(&EventRow),
) {
    let updated =
        header.with_untracked(|h| with_added_event(h, row.timestamp_ms, time_boundaries()));
    if !add_event_and_update_header_idb(db, &row, &updated).await {
        return;
    }
    header.set(updated);
    on_saved(&row);
    if let Some(gps) = get_gps().await {
        let enriched = gps.applied_to(row);
        if enrich_event_idb(db, &enriched).await {
            on_enriched(&enriched);
        }
    }
}

pub(crate) const PAGE_SIZE: usize = 50;

// ─── Context newtypes ─────────────────────────────────────────────────────────

// Newtype wrappers so Leptos context lookup never confuses same-type signals.
#[derive(Clone, Copy)]
pub(crate) struct Editing(pub(crate) RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowDetail(pub(crate) RwSignal<bool>);
#[derive(Clone, Copy)]
pub(crate) struct ShowEventDetail(pub(crate) RwSignal<bool>);

// Per-topic reactive signal list. Outer signal changes only on add/remove;
// inner RwSignal<TopicHeader> changes only when that topic's counts change.
pub(crate) type TopicList = RwSignal<Vec<RwSignal<TopicHeader>>>;

// ─── Toasts ───────────────────────────────────────────────────────────────────

const TOAST_DURATION: std::time::Duration = std::time::Duration::from_millis(4000);

/// A short message shown at the bottom of the screen, optionally with Undo.
/// The undo action is a plain `Arc<dyn Fn>` rather than a Leptos `Callback`,
/// which would be owned by (and disposed with) the scope that created it.
#[derive(Clone)]
pub(crate) struct Toast {
    pub message: String,
    pub undo: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

/// Copyable handle (provided via context) for showing toasts. A newer toast
/// replaces the current one; the generation counter keeps an older timer
/// from hiding it early.
#[derive(Clone, Copy)]
pub(crate) struct Toasts {
    current: RwSignal<Option<Toast>>,
    generation: StoredValue<u64>,
}

impl Toasts {
    fn new() -> Self {
        Toasts {
            current: RwSignal::new(None),
            generation: StoredValue::new(0),
        }
    }

    pub(crate) fn show(self, message: impl Into<String>) {
        self.push(Toast {
            message: message.into(),
            undo: None,
        });
    }

    pub(crate) fn show_with_undo(
        self,
        message: impl Into<String>,
        undo: impl Fn() + Send + Sync + 'static,
    ) {
        self.push(Toast {
            message: message.into(),
            undo: Some(std::sync::Arc::new(undo)),
        });
    }

    fn push(self, toast: Toast) {
        let generation = self.generation.get_value() + 1;
        self.generation.set_value(generation);
        self.current.set(Some(toast));
        set_timeout(
            move || {
                if self.generation.get_value() == generation {
                    self.current.set(None);
                }
            },
            TOAST_DURATION,
        );
    }

    fn dismiss(self) {
        self.current.set(None);
    }
}

#[component]
pub fn ToastBar() -> impl IntoView {
    let toasts = use_context::<Toasts>().expect("toasts context");
    let message = move || {
        toasts
            .current
            .with(|t| t.as_ref().map(|t| t.message.clone()).unwrap_or_default())
    };
    let undo = move || {
        toasts
            .current
            .with(|t| t.as_ref().and_then(|t| t.undo.clone()))
    };
    view! {
        // Always rendered so screen readers pick up changes in the live region.
        <div class="toast-region" role="status" aria-live="polite">
            <Show when=move || toasts.current.with(Option::is_some)>
                <div class="toast">
                    <span class="toast-message">{message}</span>
                    {move || undo().map(|undo| view! {
                        <button
                            class="toast-undo"
                            type="button"
                            on:click=move |_| {
                                toasts.dismiss();
                                undo();
                            }
                        >
                            "Undo"
                        </button>
                    })}
                </div>
            </Show>
        </div>
    }
}

// ─── Components ──────────────────────────────────────────────────────────────

#[component]
pub fn TopicCard(topic_signal: RwSignal<TopicHeader>) -> impl IntoView {
    let topic_list = use_context::<TopicList>().expect("topic_list context");
    let editing = use_context::<Editing>().expect("editing context").0;
    let show_detail = use_context::<ShowDetail>().expect("show_detail context").0;
    let detail_id = use_context::<RwSignal<String>>().expect("detail_id context");
    let toasts = use_context::<Toasts>().expect("toasts context");

    let add_event = move |_| {
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
                |_| {},
            )
            .await;
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
    let show_event_detail = use_context::<ShowEventDetail>()
        .expect("show_event_detail context")
        .0;
    let event_detail_ev =
        use_context::<RwSignal<Option<EventRow>>>().expect("event_detail_ev context");
    let toasts = use_context::<Toasts>().expect("toasts context");

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
                        events.update(|evs| evs.insert(0, row.clone()));
                        all_evs.update_value(|v| v.insert(0, row.clone()));
                        let event_id = row.id.clone();
                        let name = sig.with_untracked(|h| h.name.clone());
                        toasts.show_with_undo(format!("Logged in {name}"), move || {
                            let event_id = event_id.clone();
                            events.update(|evs| evs.retain(|e| e.id != event_id));
                            all_evs.update_value(|v| v.retain(|e| e.id != event_id));
                            spawn_local(async move {
                                if let Some(db) = get_db() {
                                    undo_logged_event(&db, &event_id, sig).await;
                                }
                            });
                        });
                    },
                    |row| {
                        let replace = |v: &mut Vec<EventRow>| {
                            if let Some(e) = v.iter_mut().find(|e| e.id == row.id) {
                                *e = row.clone();
                            }
                        };
                        events.update(replace);
                        all_evs.update_value(replace);
                        if event_detail_ev
                            .with_untracked(|e| e.as_ref().map(|e| &e.id) == Some(&row.id))
                        {
                            event_detail_ev.set(Some(row.clone()));
                        }
                    },
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
                                        <div class="event-item-content" on:click=open_event_detail>
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
                    <div class="header-btn" style="min-width:64px"></div>
                </div>
            </header>
            <div class="event-detail-main">
                <Show when=move || ev().is_some()>
                    {move || ev().map(|e| {
                        let ts = format_timestamp(&e.timestamp);
                        let lat_str  = e.lat.map(|v| format!("{:.6}°", v)).unwrap_or("—".into());
                        let lon_str  = e.lon.map(|v| format!("{:.6}°", v)).unwrap_or("—".into());
                        let alt_str  = fmt_opt(e.altitude, "m", 1);
                        let hdg_str  = fmt_opt(e.heading, "°", 1);
                        let spd_str  = e.speed.map(|v| format!("{:.1} m/s", v)).unwrap_or("—".into());
                        let acc_str  = fmt_opt(e.accuracy, "m ±", 1);
                        let aac_str  = fmt_opt(e.altitude_accuracy, "m ±", 1);
                        view! {
                            <div class="event-detail-card">
                                <div class="event-detail-section">
                                    <div class="event-detail-row">
                                        <span class="event-detail-label">"Time"</span>
                                        <span class="event-detail-value">{ts}</span>
                                    </div>
                                </div>
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

/// Undo a just-logged event: delete it and recompute the topic's counts from
/// what is left. Returns whether the event was deleted. A GPS fix that
/// arrives afterwards is dropped by `enrich_event_idb`.
pub(crate) async fn undo_logged_event(
    db: &Rexie,
    event_id: &str,
    header: RwSignal<TopicHeader>,
) -> bool {
    if !delete_event_idb(db, event_id).await {
        return false;
    }
    let fresh = refresh_topic_counts_idb(db, &header.get_untracked()).await;
    header.set(fresh);
    true
}

// ─── Topic signals ────────────────────────────────────────────────────────────

/// Create a topic's signal owned by `owner` (the `App`), not by whichever
/// reactive scope happens to be current. Inside an event handler that is the
/// handler's view scope, e.g. the `<Show when=adding>` form, which is disposed
/// as soon as it hides; the next access to the signal then panics.
pub(crate) fn new_topic_signal(owner: &Owner, h: TopicHeader) -> RwSignal<TopicHeader> {
    owner.with(|| RwSignal::new(h))
}

// ─── Import helper ────────────────────────────────────────────────────────────

/// What importing one topic's events did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ImportOutcome {
    pub added: usize,
    pub duplicates: usize,
}

/// Merge `incoming` into the topic called `name`, creating the topic if it
/// does not exist. Events already stored (same timestamp) are skipped; the
/// new ones get fresh ids and are written in a single transaction, after
/// which the topic's counts are recomputed. `None` if the write failed.
pub(crate) async fn import_into_topic(
    db: &Rexie,
    topic_list: TopicList,
    app_owner: StoredValue<Owner, LocalStorage>,
    name: String,
    incoming: Vec<EventRow>,
) -> Option<ImportOutcome> {
    let existing_sig = topic_list.with_untracked(|rows| {
        rows.iter()
            .find(|s| s.with_untracked(|h| h.name == name))
            .copied()
    });
    let (topic_id, existing) = match existing_sig {
        Some(sig) => {
            let id = sig.with_untracked(|h| h.id.clone());
            let events = load_events_for_topic(db, &id).await;
            (id, events)
        }
        None => (new_id(), Vec::new()),
    };

    let (mut fresh, duplicates) = merge_new_events(&existing, incoming);
    for row in &mut fresh {
        row.id = new_id();
        row.topic_id = topic_id.clone();
    }
    if !add_events_bulk_idb(db, &fresh).await {
        return None;
    }
    let added = fresh.len();
    let mut all = existing;
    all.extend(fresh);
    let counts = event_row_counts(&all, time_boundaries());

    match existing_sig {
        Some(sig) => {
            sig.update(|h| h.set_counts(counts));
            save_topic_header(db, &sig.get_untracked()).await;
        }
        None => {
            let mut header = TopicHeader::new(topic_id, name);
            header.set_counts(counts);
            save_topic_header(db, &header).await;
            let sig = app_owner.with_value(|o| new_topic_signal(o, header));
            topic_list.update(|rows| rows.push(sig));
        }
    }
    Some(ImportOutcome { added, duplicates })
}

impl std::ops::Add for ImportOutcome {
    type Output = ImportOutcome;
    fn add(self, o: ImportOutcome) -> ImportOutcome {
        ImportOutcome {
            added: self.added + o.added,
            duplicates: self.duplicates + o.duplicates,
        }
    }
}

/// "1 event", "3 events".
pub(crate) fn count_noun(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// Toast text after an import, e.g.
/// "Imported 120 events into Running (15 duplicates skipped)".
pub(crate) fn import_message(scope: &str, o: ImportOutcome) -> String {
    let mut msg = format!("Imported {} {scope}", count_noun(o.added, "event"));
    if o.duplicates > 0 {
        msg.push_str(&format!(
            " ({} skipped)",
            count_noun(o.duplicates, "duplicate")
        ));
    }
    msg
}

// ─── Foreground refresh helper ────────────────────────────────────────────────

/// Recompute and persist every topic's counts from its stored events,
/// then update the corresponding Leptos signals.
/// Called on startup and whenever the page returns to the foreground after a
/// potential day rollover.
pub(crate) async fn refresh_all_topic_counts(db: &Rexie, topic_list: TopicList) {
    for sig in topic_list.get_untracked() {
        let h = sig.get_untracked();
        let fresh = refresh_topic_counts_idb(db, &h).await;
        sig.set(fresh);
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
        let headers_raw = load_topic_headers(&db).await;
        let mut headers: Vec<TopicHeader> = Vec::new();
        for h in &headers_raw {
            headers.push(refresh_topic_counts_idb(&db, h).await);
        }

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
    });

    provide_context(topic_list);
    provide_context(toasts);
    provide_context(Editing(editing));
    provide_context(ShowDetail(show_detail));
    provide_context(detail_id);
    provide_context(ShowEventDetail(show_event_detail));
    provide_context(event_detail_ev);

    // ── Foreground detection: refresh counts when a new day has started ───────
    {
        let last_today_start = StoredValue::new(time_boundaries().today_start);
        let doc = web_sys::window().unwrap().document().unwrap();
        let doc2 = doc.clone(); // moved into the closure

        let listener = Closure::<dyn Fn()>::new(move || {
            if doc2.hidden() {
                return; // fired while going to background — nothing to do
            }
            let new_ts = time_boundaries().today_start;
            if new_ts == last_today_start.get_value() {
                return; // same day, counts are still valid
            }
            last_today_start.set_value(new_ts);
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
        let name = new_name.get().trim().to_string();
        if name.is_empty() {
            return;
        }
        let header = TopicHeader::new(new_id(), name);
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
                                title="Export all topics (JSON)"
                                on:click=on_export_all
                            >
                                "⬇"
                            </button>
                            <label class="header-btn header-btn-import" title="Import all topics (JSON)">
                                "⬆"
                                <input type="file" accept=".json" style="display:none" on:change=on_import_json />
                            </label>
                            <label class="header-btn header-btn-import" title="Import topic from .txt">
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

    /// Undo removes the event and brings the topic's counts back down.
    #[wasm_bindgen_test]
    async fn undo_logged_event_restores_counts() {
        let db = open_db().await.unwrap();
        let header = TopicHeader::new(new_id(), "undo-test".into());
        save_topic_header(&db, &header).await;
        let sig = RwSignal::new(header);

        let row = EventRow {
            id: new_id(),
            topic_id: sig.with_untracked(|h| h.id.clone()),
            timestamp: now_timestamp(),
            timestamp_ms: js_sys::Date::now(),
            ..Default::default()
        };
        record_event(&db, row.clone(), sig, |_| {}, |_| {}).await;
        assert_eq!(sig.get_untracked().count_total, 1);

        assert!(undo_logged_event(&db, &row.id, sig).await);
        assert_eq!(sig.get_untracked().counts(), Default::default());
        let topic_id = sig.with_untracked(|h| h.id.clone());
        assert!(load_events_for_topic(&db, &topic_id).await.is_empty());
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
    fn count_noun_singular_and_plural() {
        assert_eq!(count_noun(0, "topic"), "0 topics");
        assert_eq!(count_noun(1, "topic"), "1 topic");
        assert_eq!(count_noun(3, "event"), "3 events");
    }

    #[test]
    fn import_message_with_duplicates() {
        let o = ImportOutcome {
            added: 120,
            duplicates: 15,
        };
        assert_eq!(
            import_message("into Running", o),
            "Imported 120 events into Running (15 duplicates skipped)"
        );
    }

    #[test]
    fn import_message_singular() {
        let o = ImportOutcome {
            added: 1,
            duplicates: 1,
        };
        assert_eq!(
            import_message("into Yoga", o),
            "Imported 1 event into Yoga (1 duplicate skipped)"
        );
    }

    #[test]
    fn import_message_without_duplicates() {
        let o = ImportOutcome {
            added: 2,
            duplicates: 0,
        };
        assert_eq!(
            import_message("from 2 topics", o),
            "Imported 2 events from 2 topics"
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
