use rexie::{Index, KeyRange, ObjectStore, Rexie, TransactionMode};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsValue;

use crate::time::Counts;

// ─── Data model ──────────────────────────────────────────────────────────────

/// Lightweight header kept in reactive signals — no events Vec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopicHeader {
    pub id: String,
    pub name: String,
    pub count_total: u32,
    pub count_today: u32,
    pub count_week: u32,
    pub count_month: u32,
    /// Sort key for the topic list (edit mode ▲/▼). Records written before
    /// this field existed load as 0; ties are ordered by name.
    #[serde(default)]
    pub position: u32,
}

impl TopicHeader {
    /// A fresh topic with all counts at zero.
    pub(crate) fn new(id: String, name: String) -> Self {
        TopicHeader {
            id,
            name,
            count_total: 0,
            count_today: 0,
            count_week: 0,
            count_month: 0,
            position: 0,
        }
    }

    pub(crate) fn counts(&self) -> Counts {
        Counts {
            today: self.count_today,
            week: self.count_week,
            month: self.count_month,
            total: self.count_total,
        }
    }

    pub(crate) fn set_counts(&mut self, c: Counts) {
        self.count_today = c.today;
        self.count_week = c.week;
        self.count_month = c.month;
        self.count_total = c.total;
    }
}

/// Row stored in IDB "events" store.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EventRow {
    pub id: String,
    pub topic_id: String,
    pub timestamp: String,
    pub timestamp_ms: f64,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    #[serde(default)]
    pub altitude: Option<f64>,
    #[serde(default)]
    pub heading: Option<f64>,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub accuracy: Option<f64>,
    #[serde(default)]
    pub altitude_accuracy: Option<f64>,
}

// ─── Thread-local DB handle ───────────────────────────────────────────────────

// Avoids Send + Sync requirement on Rexie.
thread_local! {
    pub(crate) static DB: RefCell<Option<Rc<Rexie>>> = const { RefCell::new(None) };
}

pub(crate) fn get_db() -> Option<Rc<Rexie>> {
    DB.with(|db| db.borrow().clone())
}

// ─── IDB helpers ─────────────────────────────────────────────────────────────

pub(crate) async fn open_db() -> Result<Rexie, rexie::Error> {
    open_db_named("trackit-db", 1).await
}

async fn open_db_named(name: &str, version: u32) -> Result<Rexie, rexie::Error> {
    Rexie::builder(name)
        .version(version)
        .add_object_store(ObjectStore::new("topics").key_path("id"))
        .add_object_store(
            ObjectStore::new("events")
                .key_path("id")
                .add_index(Index::new("by_topic", "topic_id")),
        )
        .build()
        .await
}

pub(crate) async fn load_topic_headers(db: &Rexie) -> Vec<TopicHeader> {
    let tx = match db.transaction(&["topics"], TransactionMode::ReadOnly) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let store = match tx.store("topics") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let records = store.get_all(None, None).await.unwrap_or_default();
    tx.done().await.ok();
    let mut headers: Vec<TopicHeader> = records
        .into_iter()
        .filter_map(|v| serde_wasm_bindgen::from_value::<TopicHeader>(v).ok())
        .collect();
    crate::time::sort_topics(&mut headers);
    headers
}

pub(crate) async fn save_topic_header(db: &Rexie, h: &TopicHeader) {
    let tx = match db.transaction(&["topics"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return,
    };
    let store = match tx.store("topics") {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Ok(val) = serde_wasm_bindgen::to_value(h) {
        store.put(&val, None).await.ok();
    }
    tx.done().await.ok();
}

pub(crate) async fn load_events_for_topic(db: &Rexie, topic_id: &str) -> Vec<EventRow> {
    let tx = match db.transaction(&["events"], TransactionMode::ReadOnly) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let store = match tx.store("events") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let index = match store.index("by_topic") {
        Ok(i) => i,
        Err(_) => return Vec::new(),
    };
    let key_range = KeyRange::only(&JsValue::from_str(topic_id)).ok();
    let records = index.get_all(key_range, None).await.unwrap_or_default();
    tx.done().await.ok();
    let mut rows: Vec<EventRow> = records
        .into_iter()
        .filter_map(|v| serde_wasm_bindgen::from_value::<EventRow>(v).ok())
        .collect();
    rows.sort_by(|a, b| {
        b.timestamp_ms
            .partial_cmp(&a.timestamp_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

/// Save several headers (e.g. after reordering) in one transaction.
pub(crate) async fn save_topic_headers_idb(db: &Rexie, headers: &[TopicHeader]) -> bool {
    let tx = match db.transaction(&["topics"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let store = match tx.store("topics") {
        Ok(s) => s,
        Err(_) => return false,
    };
    for h in headers {
        let Ok(val) = serde_wasm_bindgen::to_value(h) else {
            return false;
        };
        if store.put(&val, None).await.is_err() {
            return false;
        }
    }
    tx.done().await.is_ok()
}

pub(crate) async fn refresh_topic_counts_idb(db: &Rexie, header: &TopicHeader) -> TopicHeader {
    let events = load_events_for_topic(db, &header.id).await;
    let mut updated = header.clone();
    updated.set_counts(crate::time::event_row_counts(
        &events,
        crate::time::time_boundaries(),
    ));
    save_topic_header(db, &updated).await;
    updated
}

/// Single-event put; production code uses the atomic / bulk variants.
#[cfg(test)]
pub(crate) async fn add_event_idb(db: &Rexie, row: &EventRow) {
    let tx = match db.transaction(&["events"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return,
    };
    let store = match tx.store("events") {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Ok(val) = serde_wasm_bindgen::to_value(row) {
        store.put(&val, None).await.ok();
    }
    tx.done().await.ok();
}

/// Write all `rows` in one readwrite transaction: either every row is stored
/// or (on failure) none is. Returns whether the transaction committed.
pub(crate) async fn add_events_bulk_idb(db: &Rexie, rows: &[EventRow]) -> bool {
    let tx = match db.transaction(&["events"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let store = match tx.store("events") {
        Ok(s) => s,
        Err(_) => return false,
    };
    for row in rows {
        let Ok(val) = serde_wasm_bindgen::to_value(row) else {
            return false;
        };
        if store.put(&val, None).await.is_err() {
            return false;
        }
    }
    tx.done().await.is_ok()
}

/// Overwrite `row` (e.g. with a GPS fix) only if an event with its id is
/// still stored. Returns whether the row was written.
pub(crate) async fn enrich_event_idb(db: &Rexie, row: &EventRow) -> bool {
    let tx = match db.transaction(&["events"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let store = match tx.store("events") {
        Ok(s) => s,
        Err(_) => return false,
    };
    // get + put in one readwrite transaction, so a delete cannot slip in between.
    if !matches!(store.get(JsValue::from_str(&row.id)).await, Ok(Some(_))) {
        return false;
    }
    let Ok(val) = serde_wasm_bindgen::to_value(row) else {
        return false;
    };
    if store.put(&val, None).await.is_err() {
        return false;
    }
    tx.done().await.is_ok()
}

pub(crate) async fn add_event_and_update_header_idb(
    db: &Rexie,
    row: &EventRow,
    header: &TopicHeader,
) -> bool {
    let tx = match db.transaction(&["events", "topics"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let (ev_store, t_store) = match (tx.store("events"), tx.store("topics")) {
        (Ok(e), Ok(t)) => (e, t),
        _ => return false,
    };
    let ev_val = match serde_wasm_bindgen::to_value(row) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if ev_store.put(&ev_val, None).await.is_err() {
        return false;
    }
    let hdr_val = match serde_wasm_bindgen::to_value(header) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if t_store.put(&hdr_val, None).await.is_err() {
        return false;
    }
    tx.done().await.is_ok()
}

/// Delete one event. Returns whether the transaction committed.
pub(crate) async fn delete_event_idb(db: &Rexie, event_id: &str) -> bool {
    let tx = match db.transaction(&["events"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let store = match tx.store("events") {
        Ok(s) => s,
        Err(_) => return false,
    };
    if store.delete(JsValue::from_str(event_id)).await.is_err() {
        return false;
    }
    tx.done().await.is_ok()
}

pub(crate) async fn delete_topic_idb(db: &Rexie, topic_id: &str) {
    let tx = match db.transaction(&["events", "topics"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return,
    };
    // Delete all events for this topic
    if let Ok(ev_store) = tx.store("events")
        && let Ok(index) = ev_store.index("by_topic")
    {
        let key_range = KeyRange::only(&JsValue::from_str(topic_id)).ok();
        if let Ok(records) = index.get_all(key_range, None).await {
            for v in records {
                if let Ok(row) = serde_wasm_bindgen::from_value::<EventRow>(v) {
                    ev_store.delete(JsValue::from_str(&row.id)).await.ok();
                }
            }
        }
    }
    if let Ok(t_store) = tx.store("topics") {
        t_store.delete(JsValue::from_str(topic_id)).await.ok();
    }
    tx.done().await.ok();
}

// ─── WASM integration tests (IDB) ────────────────────────────────────────────
//
// Run with: wasm-pack test --headless --chrome
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::{
        EventRow, TopicHeader, add_event_and_update_header_idb, add_event_idb, add_events_bulk_idb,
        delete_event_idb, delete_topic_idb, enrich_event_idb, load_events_for_topic,
        load_topic_headers, open_db, open_db_named, refresh_topic_counts_idb, save_topic_header,
        save_topic_headers_idb,
    };
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn test_header(id: &str, name: &str) -> TopicHeader {
        TopicHeader {
            id: id.into(),
            name: name.into(),
            count_total: 0,
            count_today: 0,
            count_week: 0,
            count_month: 0,
            position: 0,
        }
    }

    fn test_event(id: &str, topic_id: &str, ts_ms: f64) -> EventRow {
        EventRow {
            id: id.into(),
            topic_id: topic_id.into(),
            timestamp: "2023-11-15T12:00:00.000Z".into(),
            timestamp_ms: ts_ms,
            ..Default::default()
        }
    }

    // IDB: save a topic then load it back
    #[wasm_bindgen_test]
    async fn idb_save_and_load_topic() {
        let db = open_db().await.unwrap();
        let hdr = test_header("topic-idb-1", "Running");
        save_topic_header(&db, &hdr).await;
        let loaded = load_topic_headers(&db).await;
        assert!(
            loaded
                .iter()
                .any(|h| h.id == "topic-idb-1" && h.name == "Running")
        );
    }

    // IDB: add an event then retrieve it by topic
    #[wasm_bindgen_test]
    async fn idb_add_and_load_events() {
        let db = open_db().await.unwrap();
        let ev = test_event("ev-idb-1", "topic-idb-2", 1_700_046_000_000.0);
        save_topic_header(&db, &test_header("topic-idb-2", "Cycling")).await;
        add_event_idb(&db, &ev).await;
        let events = load_events_for_topic(&db, "topic-idb-2").await;
        assert!(events.iter().any(|e| e.id == "ev-idb-1"));
    }

    // IDB: delete an event
    #[wasm_bindgen_test]
    async fn idb_delete_event() {
        let db = open_db().await.unwrap();
        let ev = test_event("ev-idb-del", "topic-idb-3", 1_700_046_000_000.0);
        save_topic_header(&db, &test_header("topic-idb-3", "Swimming")).await;
        add_event_idb(&db, &ev).await;
        delete_event_idb(&db, "ev-idb-del").await;
        let events = load_events_for_topic(&db, "topic-idb-3").await;
        assert!(!events.iter().any(|e| e.id == "ev-idb-del"));
    }

    // IDB: deleting a topic also removes all its events
    #[wasm_bindgen_test]
    async fn idb_delete_topic_cascades() {
        let db = open_db().await.unwrap();
        save_topic_header(&db, &test_header("topic-del-1", "Yoga")).await;
        add_event_idb(&db, &test_event("ev-del-1", "topic-del-1", 1_000.0)).await;
        add_event_idb(&db, &test_event("ev-del-2", "topic-del-1", 2_000.0)).await;

        delete_topic_idb(&db, "topic-del-1").await;

        let topics = load_topic_headers(&db).await;
        assert!(!topics.iter().any(|h| h.id == "topic-del-1"));

        let events = load_events_for_topic(&db, "topic-del-1").await;
        assert!(events.is_empty());
    }

    // IDB: saving a topic header twice with the same ID overwrites it
    #[wasm_bindgen_test]
    async fn idb_save_topic_header_overwrites() {
        let db = open_db().await.unwrap();
        let mut hdr = test_header("topic-upsert-1", "Meditation");
        save_topic_header(&db, &hdr).await;

        hdr.count_total = 42;
        hdr.count_today = 3;
        save_topic_header(&db, &hdr).await;

        let topics = load_topic_headers(&db).await;
        let reloaded = topics
            .iter()
            .find(|h| h.id == "topic-upsert-1")
            .expect("topic should still exist");
        assert_eq!(reloaded.count_total, 42);
        assert_eq!(reloaded.count_today, 3);
    }

    // IDB: load_events_for_topic returns events newest-first
    #[wasm_bindgen_test]
    async fn idb_load_events_sorted_descending() {
        let db = open_db().await.unwrap();
        save_topic_header(&db, &test_header("topic-sort-1", "Running")).await;
        add_event_idb(&db, &test_event("ev-sort-1", "topic-sort-1", 1_000.0)).await;
        add_event_idb(&db, &test_event("ev-sort-2", "topic-sort-1", 3_000.0)).await;
        add_event_idb(&db, &test_event("ev-sort-3", "topic-sort-1", 2_000.0)).await;

        let events = load_events_for_topic(&db, "topic-sort-1").await;
        assert_eq!(events.len(), 3);
        assert!(
            events[0].timestamp_ms >= events[1].timestamp_ms
                && events[1].timestamp_ms >= events[2].timestamp_ms,
            "events should be in descending order by timestamp_ms"
        );
    }

    // IDB: add_event_and_update_header_idb writes event + header atomically
    #[wasm_bindgen_test]
    async fn idb_add_event_and_update_header_atomic() {
        let db = open_db().await.unwrap();

        let hdr = TopicHeader {
            id: "topic-atomic-1".into(),
            name: "Atomic test".into(),
            count_total: 0,
            count_today: 0,
            count_week: 0,
            count_month: 0,
            position: 0,
        };
        save_topic_header(&db, &hdr).await;

        let ev = test_event("ev-atomic-1", "topic-atomic-1", js_sys::Date::now());
        let updated_hdr = TopicHeader {
            count_total: 1,
            ..hdr.clone()
        };

        let ok = add_event_and_update_header_idb(&db, &ev, &updated_hdr).await;
        assert!(ok, "atomic write should succeed");

        let topics = load_topic_headers(&db).await;
        let saved = topics
            .iter()
            .find(|h| h.id == "topic-atomic-1")
            .expect("topic must still exist");
        assert_eq!(
            saved.count_total, 1,
            "count_total must be 1 after atomic write"
        );

        let events = load_events_for_topic(&db, "topic-atomic-1").await;
        assert!(
            events.iter().any(|e| e.id == "ev-atomic-1"),
            "event must be present in IDB"
        );
    }

    // IDB: refresh_topic_counts_idb overwrites stale counts with recomputed values
    #[wasm_bindgen_test]
    async fn idb_refresh_topic_counts() {
        let db = open_db().await.unwrap();

        // Save a topic with obviously wrong (stale) counts.
        let stale = TopicHeader {
            id: "topic-refresh-1".into(),
            name: "Refresh test".into(),
            count_total: 999,
            count_today: 999,
            count_week: 999,
            count_month: 999,
            position: 0,
        };
        save_topic_header(&db, &stale).await;

        // Add exactly one event timestamped right now.
        let ev = EventRow {
            id: "ev-refresh-1".into(),
            topic_id: "topic-refresh-1".into(),
            timestamp: crate::time::now_timestamp(),
            timestamp_ms: js_sys::Date::now(),
            ..Default::default()
        };
        add_event_idb(&db, &ev).await;

        // Refresh recomputes counts from actual events.
        let refreshed = refresh_topic_counts_idb(&db, &stale).await;

        assert_eq!(refreshed.count_total, 1, "total must be 1 after refresh");
        assert_eq!(refreshed.count_today, 1, "today must be 1 after refresh");
        assert_eq!(refreshed.count_week, 1, "week must be 1 after refresh");
        assert_eq!(refreshed.count_month, 1, "month must be 1 after refresh");

        // The persisted value must also be corrected.
        let headers = load_topic_headers(&db).await;
        let saved = headers
            .iter()
            .find(|h| h.id == "topic-refresh-1")
            .expect("topic must still exist");
        assert_eq!(
            saved.count_today, 1,
            "persisted count_today must be corrected"
        );
        assert_eq!(
            saved.count_total, 1,
            "persisted count_total must be corrected"
        );
    }

    // IDB: enriching an event that was deleted meanwhile must not bring it back
    #[wasm_bindgen_test]
    async fn idb_enrich_does_not_resurrect_deleted_event() {
        let db = open_db().await.unwrap();
        let ev = test_event("ev-enrich-del", "topic-enrich-1", 1_000.0);
        add_event_idb(&db, &ev).await;
        delete_event_idb(&db, "ev-enrich-del").await;

        let enriched = EventRow {
            lat: Some(48.1),
            ..ev
        };
        assert!(!enrich_event_idb(&db, &enriched).await, "nothing to enrich");

        let events = load_events_for_topic(&db, "topic-enrich-1").await;
        assert!(
            !events.iter().any(|e| e.id == "ev-enrich-del"),
            "deleted event must stay deleted"
        );
    }

    // IDB: enriching an existing event overwrites it in place
    #[wasm_bindgen_test]
    async fn idb_enrich_updates_existing_event() {
        let db = open_db().await.unwrap();
        let ev = test_event("ev-enrich-upd", "topic-enrich-2", 1_000.0);
        add_event_idb(&db, &ev).await;

        let enriched = EventRow {
            lat: Some(48.1),
            lon: Some(11.5),
            ..ev
        };
        assert!(enrich_event_idb(&db, &enriched).await);

        let events = load_events_for_topic(&db, "topic-enrich-2").await;
        assert_eq!(events.len(), 1);
        assert_eq!((events[0].lat, events[0].lon), (Some(48.1), Some(11.5)));
    }

    // IDB: an open that IndexedDB rejects is reported as Err instead of panicking
    #[wasm_bindgen_test]
    async fn idb_open_failure_returns_err() {
        // Opening an existing database at a lower version is always a VersionError.
        open_db_named("trackit-test-version", 2)
            .await
            .expect("first open succeeds")
            .close();
        assert!(open_db_named("trackit-test-version", 1).await.is_err());
    }

    // IDB: bulk insert stores every row in one go
    #[wasm_bindgen_test]
    async fn idb_add_events_bulk_writes_all() {
        let db = open_db().await.unwrap();
        let rows: Vec<EventRow> = (0..50)
            .map(|i| test_event(&format!("ev-bulk-{i}"), "topic-bulk-1", i as f64))
            .collect();
        assert!(
            add_events_bulk_idb(&db, &rows).await,
            "transaction should commit"
        );
        let events = load_events_for_topic(&db, "topic-bulk-1").await;
        assert_eq!(events.len(), 50);
    }

    // IDB: bulk insert of nothing is a successful no-op
    #[wasm_bindgen_test]
    async fn idb_add_events_bulk_empty_is_ok() {
        let db = open_db().await.unwrap();
        assert!(add_events_bulk_idb(&db, &[]).await);
    }

    // IDB: positions saved in one batch come back in that order
    #[wasm_bindgen_test]
    async fn idb_save_topic_headers_persists_positions() {
        let db = open_db_named("trackit-test-order", 1).await.unwrap();
        let mut headers = vec![
            TopicHeader {
                position: 2,
                ..test_header("o-1", "A")
            },
            TopicHeader {
                position: 0,
                ..test_header("o-2", "B")
            },
            TopicHeader {
                position: 1,
                ..test_header("o-3", "C")
            },
        ];
        assert!(save_topic_headers_idb(&db, &headers).await);
        let loaded: Vec<String> = load_topic_headers(&db)
            .await
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(loaded, ["o-2", "o-3", "o-1"]);

        // Moving again overwrites the stored positions.
        headers[0].position = 0;
        headers[1].position = 1;
        assert!(save_topic_headers_idb(&db, &headers[..2]).await);
        let loaded: Vec<String> = load_topic_headers(&db)
            .await
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(loaded, ["o-1", "o-2", "o-3"]);
    }
}
