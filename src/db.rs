use rexie::{Index, KeyRange, ObjectStore, Rexie, TransactionMode};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use wasm_bindgen::JsValue;

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
}

/// Row stored in IDB "events" store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventRow {
    pub id: String,
    pub topic_id: String,
    pub timestamp: String,
    pub timestamp_ms: f64,
}

// ─── Thread-local DB handle ───────────────────────────────────────────────────

// Avoids Send + Sync requirement on Rexie.
thread_local! {
    pub(crate) static DB: RefCell<Option<Rexie>> = const { RefCell::new(None) };
}

pub(crate) fn get_db() -> Option<Rexie> {
    DB.with(|db| db.borrow().clone())
}

// ─── IDB helpers ─────────────────────────────────────────────────────────────

pub(crate) async fn open_db() -> Rexie {
    Rexie::builder("trackit-db")
        .version(1)
        .add_object_store(ObjectStore::new("topics").key_path("id"))
        .add_object_store(
            ObjectStore::new("events")
                .key_path("id")
                .add_index(Index::new("by_topic", "topic_id")),
        )
        .build()
        .await
        .expect("IDB open failed")
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
    let records = store
        .get_all(None, None, None, None)
        .await
        .unwrap_or_default();
    tx.done().await.ok();
    records
        .into_iter()
        .filter_map(|(_k, v)| serde_wasm_bindgen::from_value::<TopicHeader>(v).ok())
        .collect()
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
    let records = index
        .get_all(key_range.as_ref(), None, None, None)
        .await
        .unwrap_or_default();
    tx.done().await.ok();
    let mut rows: Vec<EventRow> = records
        .into_iter()
        .filter_map(|(_k, v)| serde_wasm_bindgen::from_value::<EventRow>(v).ok())
        .collect();
    rows.sort_by(|a, b| {
        b.timestamp_ms
            .partial_cmp(&a.timestamp_ms)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    rows
}

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

pub(crate) async fn delete_event_idb(db: &Rexie, event_id: &str) {
    let tx = match db.transaction(&["events"], TransactionMode::ReadWrite) {
        Ok(t) => t,
        Err(_) => return,
    };
    let store = match tx.store("events") {
        Ok(s) => s,
        Err(_) => return,
    };
    store.delete(&JsValue::from_str(event_id)).await.ok();
    tx.done().await.ok();
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
        if let Ok(records) = index.get_all(key_range.as_ref(), None, None, None).await {
            for (_k, v) in records {
                if let Ok(row) = serde_wasm_bindgen::from_value::<EventRow>(v) {
                    ev_store.delete(&JsValue::from_str(&row.id)).await.ok();
                }
            }
        }
    }
    if let Ok(t_store) = tx.store("topics") {
        t_store.delete(&JsValue::from_str(topic_id)).await.ok();
    }
    tx.done().await.ok();
}

// ─── WASM integration tests (IDB) ────────────────────────────────────────────
//
// Run with: wasm-pack test --headless --chrome
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::{
        EventRow, TopicHeader, add_event_idb, delete_event_idb, delete_topic_idb,
        load_events_for_topic, load_topic_headers, open_db, save_topic_header,
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
        }
    }

    fn test_event(id: &str, topic_id: &str, ts_ms: f64) -> EventRow {
        EventRow {
            id: id.into(),
            topic_id: topic_id.into(),
            timestamp: "2023-11-15T12:00:00.000Z".into(),
            timestamp_ms: ts_ms,
        }
    }

    // IDB: save a topic then load it back
    #[wasm_bindgen_test]
    async fn idb_save_and_load_topic() {
        let db = open_db().await;
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
        let db = open_db().await;
        let ev = test_event("ev-idb-1", "topic-idb-2", 1_700_046_000_000.0);
        save_topic_header(&db, &test_header("topic-idb-2", "Cycling")).await;
        add_event_idb(&db, &ev).await;
        let events = load_events_for_topic(&db, "topic-idb-2").await;
        assert!(events.iter().any(|e| e.id == "ev-idb-1"));
    }

    // IDB: delete an event
    #[wasm_bindgen_test]
    async fn idb_delete_event() {
        let db = open_db().await;
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
        let db = open_db().await;
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
        let db = open_db().await;
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
        let db = open_db().await;
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
}
