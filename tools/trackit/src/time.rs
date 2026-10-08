use crate::db::{EventRow, TopicHeader};
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::window;

// ─── Utilities ────────────────────────────────────────────────────────────────

pub(crate) fn now_timestamp() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}

pub(crate) fn now_local_datetime_str() -> String {
    let d = js_sys::Date::new_0();
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds(),
    )
}

pub(crate) fn format_timestamp(iso: &str) -> String {
    let d = js_sys::Date::new(&JsValue::from_str(iso));
    format!(
        "{:02}.{:02}.{} - {:02}:{:02}:{:02}",
        d.get_date(),
        d.get_month() + 1,
        d.get_full_year(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds(),
    )
}

pub(crate) fn new_id() -> String {
    let ts = js_sys::Date::now() as u64;
    let rand = (js_sys::Math::random() * 1_000_000.0) as u64;
    format!("{}-{}", ts, rand)
}

/// Period boundaries (epoch ms) used to bucket events into today / week / month.
/// `week_start` is a rolling 7-day window; `month_start` is the calendar month.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bounds {
    pub now: f64,
    pub today_start: f64,
    pub today_end: f64,
    pub month_start: f64,
    pub week_start: f64,
}

/// Per-period event counts, as stored denormalized in `TopicHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Counts {
    pub today: u32,
    pub week: u32,
    pub month: u32,
    pub total: u32,
}

impl Counts {
    /// Contribution of a single event at `ms` under `b`.
    fn of_event(ms: f64, b: Bounds) -> Self {
        Counts {
            today: (ms >= b.today_start && ms < b.today_end) as u32,
            week: (ms >= b.week_start && ms <= b.now) as u32,
            month: (ms >= b.month_start) as u32,
            total: 1,
        }
    }
}

impl std::ops::Add for Counts {
    type Output = Counts;
    fn add(self, o: Counts) -> Counts {
        Counts {
            today: self.today + o.today,
            week: self.week + o.week,
            month: self.month + o.month,
            total: self.total + o.total,
        }
    }
}

pub(crate) fn time_boundaries() -> Bounds {
    let now = js_sys::Date::new_0();
    let now_ms = now.get_time();
    let (cy, cm, cd) = (now.get_full_year(), now.get_month() + 1, now.get_date());
    let today_start = js_sys::Date::new(&JsValue::from_str(&format!(
        "{}-{:02}-{:02}T00:00:00",
        cy, cm, cd
    )))
    .get_time();
    let month_start =
        js_sys::Date::new(&JsValue::from_str(&format!("{}-{:02}-01T00:00:00", cy, cm))).get_time();
    Bounds {
        now: now_ms,
        today_start,
        today_end: today_start + 86_400_000.0,
        month_start,
        week_start: now_ms - 7.0 * 86_400_000.0,
    }
}

pub(crate) fn event_row_counts(events: &[EventRow], b: Bounds) -> Counts {
    events.iter().fold(Counts::default(), |acc, ev| {
        acc + Counts::of_event(ev.timestamp_ms, b)
    })
}

/// `h` with its counts bumped for one newly logged event at `ms`.
pub(crate) fn with_added_event(h: &TopicHeader, ms: f64, b: Bounds) -> TopicHeader {
    let mut updated = h.clone();
    updated.set_counts(h.counts() + Counts::of_event(ms, b));
    updated
}

pub(crate) fn parse_import_line(line: &str) -> Option<EventRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (date_part, time_part) = line.split_once(' ')?;
    let (hms, frac) = time_part.split_once('.').unwrap_or((time_part, "000"));
    let ms_str = format!("{:0<3}", frac);
    let ms_part = &ms_str[..ms_str.len().min(3)];
    let local_iso = format!("{}T{}.{}", date_part, hms, ms_part);
    let d = js_sys::Date::new(&JsValue::from_str(&local_iso));
    if d.get_time().is_nan() {
        return None;
    }
    let ts_ms = d.get_time();
    Some(EventRow {
        id: new_id(),
        topic_id: String::new(), // filled by caller
        timestamp: d.to_iso_string().as_string()?,
        timestamp_ms: ts_ms,
        ..Default::default()
    })
}

pub(crate) fn export_topic(name: &str, events: &[EventRow]) {
    let mut sorted: Vec<&EventRow> = events.iter().collect();
    sorted.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

    let content: String = sorted
        .iter()
        .map(|ev| {
            let d = js_sys::Date::new(&JsValue::from_str(&ev.timestamp));
            format!(
                "{}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}000\n",
                d.get_full_year(),
                d.get_month() + 1,
                d.get_date(),
                d.get_hours(),
                d.get_minutes(),
                d.get_seconds(),
                d.get_milliseconds(),
            )
        })
        .collect();

    let arr = js_sys::Array::new();
    arr.push(&JsValue::from_str(&content));
    let blob = web_sys::Blob::new_with_str_sequence(&arr).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

    let doc = window().unwrap().document().unwrap();
    let a: web_sys::HtmlAnchorElement = doc.create_element("a").unwrap().dyn_into().unwrap();
    a.set_href(&url);
    a.set_download(&format!("{}.txt", name));
    doc.body().unwrap().append_child(&a).unwrap();
    a.click();
    doc.body().unwrap().remove_child(&a).unwrap();
    web_sys::Url::revoke_object_url(&url).unwrap();
}

// ─── Bulk export / import ────────────────────────────────────────────────────

/// One topic together with all of its events, used in the JSON backup format.
#[derive(Serialize, Deserialize)]
pub(crate) struct TopicExport {
    pub id: String,
    pub name: String,
    pub events: Vec<EventRow>,
}

/// Top-level JSON backup document.
#[derive(Serialize, Deserialize)]
pub(crate) struct BulkExport {
    pub version: u32,
    pub topics: Vec<TopicExport>,
}

/// Serialize all topics+events to JSON and trigger a browser download.
pub(crate) fn export_all(topics: &[(TopicHeader, Vec<EventRow>)]) {
    let export = BulkExport {
        version: BULK_EXPORT_VERSION,
        topics: topics
            .iter()
            .map(|(h, evs)| TopicExport {
                id: h.id.clone(),
                name: h.name.clone(),
                events: evs.clone(),
            })
            .collect(),
    };
    let json = match serde_json::to_string(&export) {
        Ok(j) => j,
        Err(_) => return,
    };
    let now = now_local_datetime_str();
    let date = &now[..10]; // YYYY-MM-DD
    let arr = js_sys::Array::new();
    arr.push(&JsValue::from_str(&json));
    let blob = web_sys::Blob::new_with_str_sequence(&arr).unwrap();
    let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();
    let doc = window().unwrap().document().unwrap();
    let a: web_sys::HtmlAnchorElement = doc.create_element("a").unwrap().dyn_into().unwrap();
    a.set_href(&url);
    a.set_download(&format!("trackit-{}.json", date));
    doc.body().unwrap().append_child(&a).unwrap();
    a.click();
    doc.body().unwrap().remove_child(&a).unwrap();
    web_sys::Url::revoke_object_url(&url).unwrap();
}

/// The only backup format version this build understands.
pub(crate) const BULK_EXPORT_VERSION: u32 = 1;

/// Deserialize a bulk-export JSON string; returns `None` on any parse error
/// or an unknown format version.
pub(crate) fn parse_bulk_import(json: &str) -> Option<BulkExport> {
    serde_json::from_str::<BulkExport>(json)
        .ok()
        .filter(|b| b.version == BULK_EXPORT_VERSION)
}

// ─── Unit tests ───────────────────────────────────────────────────────────────
//
// Run with: cargo test  (native target — no WASM toolchain needed)
//
// time_boundaries() is WASM-only (uses js_sys::Date), so tests construct the
// bounds tuple manually with known epoch-millisecond values.
#[cfg(test)]
mod tests {
    use super::{
        Bounds, BulkExport, Counts, TopicExport, event_row_counts, parse_bulk_import,
        with_added_event,
    };
    use crate::db::{EventRow, TopicHeader};

    // 2023-11-15 12:00:00 UTC  →  1_700_046_000_000 ms since epoch
    const NOW: f64 = 1_700_046_000_000.0;
    // 2023-11-15 00:00:00 UTC
    const TODAY_START: f64 = 1_700_006_400_000.0;
    const TODAY_END: f64 = TODAY_START + 86_400_000.0;
    // rolling 7 days
    const WEEK_START: f64 = NOW - 7.0 * 86_400_000.0;
    // 2023-11-01 00:00:00 UTC
    const MONTH_START: f64 = 1_698_796_800_000.0;

    fn bounds() -> Bounds {
        Bounds {
            now: NOW,
            today_start: TODAY_START,
            today_end: TODAY_END,
            month_start: MONTH_START,
            week_start: WEEK_START,
        }
    }

    fn counts(today: u32, week: u32, month: u32, total: u32) -> Counts {
        Counts {
            today,
            week,
            month,
            total,
        }
    }

    fn ev(ts_ms: f64) -> EventRow {
        EventRow {
            id: "x".into(),
            topic_id: "t".into(),
            timestamp: "".into(),
            timestamp_ms: ts_ms,
            ..Default::default()
        }
    }

    #[test]
    fn empty_events_all_zero() {
        assert_eq!(event_row_counts(&[], bounds()), Counts::default());
    }

    #[test]
    fn event_in_today_counts_all_periods() {
        // An event timestamped at noon today is inside today, week, and month.
        let events = vec![ev(NOW)];
        assert_eq!(event_row_counts(&events, bounds()), counts(1, 1, 1, 1));
    }

    #[test]
    fn event_yesterday_not_today_but_in_week_and_month() {
        let yesterday = NOW - 86_400_000.0; // 24 h ago, inside 7-day window
        let events = vec![ev(yesterday)];
        assert_eq!(event_row_counts(&events, bounds()), counts(0, 1, 1, 1));
    }

    #[test]
    fn event_eight_days_ago_only_in_month() {
        let old = NOW - 8.0 * 86_400_000.0; // outside 7-day window, inside month
        let events = vec![ev(old)];
        assert_eq!(event_row_counts(&events, bounds()), counts(0, 0, 1, 1));
    }

    #[test]
    fn event_before_month_start_only_in_total() {
        let ancient = MONTH_START - 1.0;
        let events = vec![ev(ancient)];
        assert_eq!(event_row_counts(&events, bounds()), counts(0, 0, 0, 1));
    }

    #[test]
    fn mixed_events_correct_counts() {
        let events = vec![
            ev(NOW),                      // today + week + month
            ev(NOW - 86_400_000.0),       // week + month
            ev(NOW - 8.0 * 86_400_000.0), // month only
            ev(MONTH_START - 1.0),        // none
        ];
        assert_eq!(event_row_counts(&events, bounds()), counts(1, 2, 3, 4));
    }

    #[test]
    fn stale_boundaries_miscount_crossed_day() {
        // Event timestamped 1 hour before midnight yesterday.
        let event_ts = TODAY_START - 3_600_000.0; // yesterday 23:00
        let events = vec![ev(event_ts)];

        // With current (today's) boundaries → NOT today, but still in the week window.
        let c = event_row_counts(&events, bounds());
        assert_eq!(c.today, 0, "event from yesterday must not count as today");
        assert_eq!(c.week, 1, "but it is still within the 7-day window");
        assert_eq!(c.month, 1);
        assert_eq!(c.total, 1);

        // With yesterday's boundaries (simulating stored stale counts) → WAS today.
        let stale = Bounds {
            now: TODAY_START - 1.0,                  // 23:59:59 yesterday
            today_start: TODAY_START - 86_400_000.0, // yesterday midnight
            today_end: TODAY_START,                  // today midnight
            month_start: MONTH_START,
            week_start: TODAY_START - 86_400_000.0 - 7.0 * 86_400_000.0,
        };
        let today_stale = event_row_counts(&events, stale).today;
        assert_eq!(
            today_stale, 1,
            "same event WAS counted as today under yesterday's stale boundaries"
        );
    }

    fn header_with(c: Counts) -> TopicHeader {
        let mut h = TopicHeader::new("t".into(), "Running".into());
        h.set_counts(c);
        h
    }

    #[test]
    fn topic_header_counts_round_trip() {
        let mut h = TopicHeader::new("t".into(), "Running".into());
        assert_eq!(h.counts(), Counts::default());
        h.set_counts(counts(1, 2, 3, 4));
        assert_eq!(h.counts(), counts(1, 2, 3, 4));
        assert_eq!(
            (h.count_today, h.count_week, h.count_month, h.count_total),
            (1, 2, 3, 4)
        );
    }

    #[test]
    fn with_added_event_now_bumps_every_period() {
        let h = with_added_event(&header_with(counts(1, 2, 3, 4)), NOW, bounds());
        assert_eq!(h.counts(), counts(2, 3, 4, 5));
    }

    #[test]
    fn with_added_event_yesterday_skips_today() {
        let h = with_added_event(
            &header_with(Counts::default()),
            NOW - 86_400_000.0,
            bounds(),
        );
        assert_eq!(h.counts(), counts(0, 1, 1, 1));
    }

    #[test]
    fn with_added_event_future_counts_month_but_not_week() {
        // A manually logged event two days ahead is outside the rolling
        // 7-day window (which ends at now) but still inside this month.
        let h = with_added_event(
            &header_with(Counts::default()),
            NOW + 2.0 * 86_400_000.0,
            bounds(),
        );
        assert_eq!(h.counts(), counts(0, 0, 1, 1));
    }

    #[test]
    fn with_added_event_keeps_identity() {
        let h = with_added_event(&header_with(Counts::default()), NOW, bounds());
        assert_eq!((h.id.as_str(), h.name.as_str()), ("t", "Running"));
    }

    #[test]
    fn topic_header_serde_round_trip() {
        let h = TopicHeader {
            id: "abc".into(),
            name: "Running".into(),
            count_total: 42,
            count_today: 1,
            count_week: 5,
            count_month: 10,
        };
        let json = serde_json::to_string(&h).unwrap();
        let h2: TopicHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(h, h2);
    }

    #[test]
    fn bulk_export_serde_round_trip() {
        let ev = EventRow {
            id: "e1".into(),
            topic_id: "t1".into(),
            timestamp: "2023-11-15T12:00:00.000Z".into(),
            timestamp_ms: NOW,
            ..Default::default()
        };
        let export = BulkExport {
            version: 1,
            topics: vec![TopicExport {
                id: "t1".into(),
                name: "Running".into(),
                events: vec![ev],
            }],
        };
        let json = serde_json::to_string(&export).unwrap();
        let back: BulkExport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, 1);
        assert_eq!(back.topics.len(), 1);
        assert_eq!(back.topics[0].name, "Running");
        assert_eq!(back.topics[0].events.len(), 1);
        assert_eq!(back.topics[0].events[0].id, "e1");
    }

    #[test]
    fn parse_bulk_import_valid() {
        let json = r#"{"version":1,"topics":[{"id":"t1","name":"Running","events":[]}]}"#;
        let result = parse_bulk_import(json);
        assert!(result.is_some());
        let bulk = result.unwrap();
        assert_eq!(bulk.topics[0].name, "Running");
    }

    #[test]
    fn parse_bulk_import_rejects_unknown_version() {
        let json = r#"{"version":2,"topics":[{"id":"t1","name":"Running","events":[]}]}"#;
        assert!(parse_bulk_import(json).is_none());
    }

    #[test]
    fn parse_bulk_import_invalid_returns_none() {
        assert!(parse_bulk_import("not json").is_none());
        assert!(parse_bulk_import("{}").is_none()); // missing required fields
    }

    #[test]
    fn event_row_serde_round_trip() {
        let e = EventRow {
            id: "e1".into(),
            topic_id: "t1".into(),
            timestamp: "2023-11-15T12:00:00.000Z".into(),
            timestamp_ms: NOW,
            ..Default::default()
        };
        let json = serde_json::to_string(&e).unwrap();
        let e2: EventRow = serde_json::from_str(&json).unwrap();
        assert_eq!(e, e2);
    }
}

// ─── WASM integration tests (parse utilities + js_sys functions) ──────────────
//
// Run with: wasm-pack test --headless --chrome
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::{
        export_topic, format_timestamp, new_id, now_local_datetime_str, now_timestamp,
        parse_import_line, time_boundaries,
    };
    use crate::db::EventRow;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    // parse_import_line: valid line parses successfully
    #[wasm_bindgen_test]
    fn parse_valid_import_line() {
        let row = parse_import_line("2023-11-15 12:00:00.123000")
            .expect("should parse a valid timestamp line");
        assert!(row.timestamp_ms > 0.0);
        assert!(!row.timestamp.is_empty());
        assert_eq!(row.topic_id, ""); // caller fills this in
    }

    // parse_import_line: empty / blank lines return None
    #[wasm_bindgen_test]
    fn parse_empty_import_line_returns_none() {
        assert!(parse_import_line("").is_none());
        assert!(parse_import_line("   ").is_none());
    }

    // parse_import_line: malformed line returns None
    #[wasm_bindgen_test]
    fn parse_malformed_import_line_returns_none() {
        assert!(parse_import_line("not-a-date").is_none());
        assert!(parse_import_line("9999-99-99 99:99:99.000").is_none());
    }

    // format_timestamp: output shape is DD.MM.YYYY - HH:MM:SS (21 chars)
    #[wasm_bindgen_test]
    fn format_timestamp_has_expected_shape() {
        let s = format_timestamp("2023-11-15T12:30:45.000Z");
        assert_eq!(s.len(), 21, "unexpected length: {s}");
        assert_eq!(&s[2..3], ".");
        assert_eq!(&s[5..6], ".");
        assert_eq!(&s[10..13], " - ");
    }

    #[wasm_bindgen_test]
    fn format_timestamp_shape_is_stable_for_any_valid_iso() {
        let s = format_timestamp("2000-01-01T00:00:00.000Z");
        assert_eq!(s.len(), 21);
        assert_eq!(&s[2..3], ".");
        assert_eq!(&s[10..13], " - ");
    }

    // now_timestamp: returns a non-empty ISO 8601 UTC string
    #[wasm_bindgen_test]
    fn now_timestamp_returns_iso_utc_string() {
        let ts = now_timestamp();
        assert!(!ts.is_empty());
        assert!(ts.contains('T'), "expected ISO format, got: {ts}");
        assert!(ts.ends_with('Z'), "expected UTC 'Z' suffix, got: {ts}");
    }

    // now_local_datetime_str: shape is YYYY-MM-DDTHH:MM:SS (19 chars)
    #[wasm_bindgen_test]
    fn now_local_datetime_str_has_expected_shape() {
        let s = now_local_datetime_str();
        assert_eq!(s.len(), 19, "unexpected length: {s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "T");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
    }

    // new_id: format is {digits}-{digits}
    #[wasm_bindgen_test]
    fn new_id_has_numeric_dash_numeric_format() {
        let id = new_id();
        let mut parts = id.splitn(2, '-');
        let ts_part = parts.next().expect("missing ts part");
        let rand_part = parts.next().expect("missing rand part");
        assert!(
            ts_part.chars().all(|c| c.is_ascii_digit()),
            "ts not numeric: {ts_part}"
        );
        assert!(
            rand_part.chars().all(|c| c.is_ascii_digit()),
            "rand not numeric: {rand_part}"
        );
    }

    #[wasm_bindgen_test]
    fn new_id_is_unique_across_calls() {
        let a = new_id();
        let b = new_id();
        assert_ne!(a, b);
    }

    // time_boundaries: structural invariants
    #[wasm_bindgen_test]
    fn time_boundaries_ordering_invariants() {
        let b = time_boundaries();
        assert!(b.now > 0.0);
        assert!(b.today_start <= b.now, "today_start should be <= now");
        assert!(b.now < b.today_end, "now should be < today_end");
        assert!(b.month_start <= b.now, "month_start should be <= now");
        assert!(b.week_start <= b.now, "week_start should be <= now");
    }

    #[wasm_bindgen_test]
    fn time_boundaries_today_span_is_exactly_one_day() {
        let b = time_boundaries();
        assert_eq!(b.today_end - b.today_start, 86_400_000.0);
    }

    #[wasm_bindgen_test]
    fn time_boundaries_week_start_is_seven_days_before_now() {
        let b = time_boundaries();
        assert_eq!(b.now - b.week_start, 7.0 * 86_400_000.0);
    }

    // export_topic: smoke test — must not panic with empty or non-empty input
    #[wasm_bindgen_test]
    fn export_topic_does_not_panic() {
        export_topic("smoke-test", &[]);
        let ev = EventRow {
            id: "e1".into(),
            topic_id: "t1".into(),
            timestamp: "2023-11-15T12:00:00.000Z".into(),
            timestamp_ms: 1_700_046_000_000.0,
            ..Default::default()
        };
        export_topic("smoke-test-2", &[ev]);
    }
}
