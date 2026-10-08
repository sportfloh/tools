//! Logging an event: GPS snapshot, the atomic write, Undo.

use crate::db::{
    EventRow, GpsFix, TopicHeader, add_event_and_update_header_idb, delete_event_idb,
    enrich_gps_idb, refresh_topic_counts_idb,
};
use crate::time::{time_boundaries, with_added_event};
use leptos::prelude::*;
use rexie::Rexie;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

// ─── GPS snapshot ─────────────────────────────────────────────────────────────

async fn get_gps() -> Option<GpsFix> {
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
    Some(GpsFix {
        lat,
        lon,
        altitude,
        heading,
        speed,
        accuracy,
        altitude_accuracy,
    })
}

/// Log `row` for the topic in `header`: persist the event together with the
/// bumped counts, then attach a GPS fix in the background once one arrives.
/// `on_saved` runs after the event has been persisted, `on_enriched` after the
/// GPS fix has been written. The fix is dropped if the event was deleted while
/// waiting for it.
pub(crate) async fn record_event(
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
    if let Some(gps) = get_gps().await
        && let Some(enriched) = enrich_gps_idb(db, &row.id, &gps).await
    {
        on_enriched(&enriched);
    }
}

/// Undo a just-logged event: delete it and recompute the topic's counts from
/// what is left. Returns whether the event was deleted. A GPS fix that
/// arrives afterwards is dropped by `update_event_idb`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{load_events_for_topic, open_db, save_topic_header};
    use crate::time::{new_id, now_timestamp};
    use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
    wasm_bindgen_test_configure!(run_in_browser);

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
}
