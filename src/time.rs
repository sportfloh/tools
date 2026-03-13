use crate::db::EventRow;
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

pub(crate) fn time_boundaries() -> (f64, f64, f64, f64, f64) {
    let now = js_sys::Date::new_0();
    let now_ms = now.get_time();
    let (cy, cm, cd) = (now.get_full_year(), now.get_month() + 1, now.get_date());
    let today_start = js_sys::Date::new(&JsValue::from_str(&format!(
        "{}-{:02}-{:02}T00:00:00",
        cy, cm, cd
    )))
    .get_time();
    let today_end = today_start + 86_400_000.0;
    let month_start =
        js_sys::Date::new(&JsValue::from_str(&format!("{}-{:02}-01T00:00:00", cy, cm))).get_time();
    let week_start = now_ms - 7.0 * 86_400_000.0;
    (now_ms, today_start, today_end, month_start, week_start)
}

pub(crate) fn event_row_counts(
    events: &[EventRow],
    (now_ms, today_start, today_end, month_start, week_start): (f64, f64, f64, f64, f64),
) -> (u32, u32, u32, u32) {
    let (t, w, m) = events.iter().fold((0u32, 0u32, 0u32), |(t, w, m), ev| {
        let ms = ev.timestamp_ms;
        (
            t + (ms >= today_start && ms < today_end) as u32,
            w + (ms >= week_start && ms <= now_ms) as u32,
            m + (ms >= month_start) as u32,
        )
    });
    (t, w, m, events.len() as u32)
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

// ─── Unit tests ───────────────────────────────────────────────────────────────
//
// Run with: cargo test  (native target — no WASM toolchain needed)
//
// time_boundaries() is WASM-only (uses js_sys::Date), so tests construct the
// bounds tuple manually with known epoch-millisecond values.
#[cfg(test)]
mod tests {
    use super::event_row_counts;
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

    fn bounds() -> (f64, f64, f64, f64, f64) {
        (NOW, TODAY_START, TODAY_END, MONTH_START, WEEK_START)
    }

    fn ev(ts_ms: f64) -> EventRow {
        EventRow {
            id: "x".into(),
            topic_id: "t".into(),
            timestamp: "".into(),
            timestamp_ms: ts_ms,
        }
    }

    #[test]
    fn empty_events_all_zero() {
        let (today, week, month, total) = event_row_counts(&[], bounds());
        assert_eq!((today, week, month, total), (0, 0, 0, 0));
    }

    #[test]
    fn event_in_today_counts_all_periods() {
        // An event timestamped at noon today is inside today, week, and month.
        let events = vec![ev(NOW)];
        let (today, week, month, total) = event_row_counts(&events, bounds());
        assert_eq!(today, 1);
        assert_eq!(week, 1);
        assert_eq!(month, 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn event_yesterday_not_today_but_in_week_and_month() {
        let yesterday = NOW - 86_400_000.0; // 24 h ago, inside 7-day window
        let events = vec![ev(yesterday)];
        let (today, week, month, total) = event_row_counts(&events, bounds());
        assert_eq!(today, 0);
        assert_eq!(week, 1);
        assert_eq!(month, 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn event_eight_days_ago_only_in_month() {
        let old = NOW - 8.0 * 86_400_000.0; // outside 7-day window, inside month
        let events = vec![ev(old)];
        let (today, week, month, total) = event_row_counts(&events, bounds());
        assert_eq!(today, 0);
        assert_eq!(week, 0);
        assert_eq!(month, 1);
        assert_eq!(total, 1);
    }

    #[test]
    fn event_before_month_start_only_in_total() {
        let ancient = MONTH_START - 1.0;
        let events = vec![ev(ancient)];
        let (today, week, month, total) = event_row_counts(&events, bounds());
        assert_eq!(today, 0);
        assert_eq!(week, 0);
        assert_eq!(month, 0);
        assert_eq!(total, 1);
    }

    #[test]
    fn mixed_events_correct_counts() {
        let events = vec![
            ev(NOW),                      // today + week + month
            ev(NOW - 86_400_000.0),       // week + month
            ev(NOW - 8.0 * 86_400_000.0), // month only
            ev(MONTH_START - 1.0),        // none
        ];
        let (today, week, month, total) = event_row_counts(&events, bounds());
        assert_eq!(today, 1);
        assert_eq!(week, 2);
        assert_eq!(month, 3);
        assert_eq!(total, 4);
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
    fn event_row_serde_round_trip() {
        let e = EventRow {
            id: "e1".into(),
            topic_id: "t1".into(),
            timestamp: "2023-11-15T12:00:00.000Z".into(),
            timestamp_ms: NOW,
        };
        let json = serde_json::to_string(&e).unwrap();
        let e2: EventRow = serde_json::from_str(&json).unwrap();
        assert_eq!(e, e2);
    }
}

// ─── WASM integration tests (parse utilities) ─────────────────────────────────
//
// Run with: wasm-pack test --headless --chrome
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::parse_import_line;
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
}
