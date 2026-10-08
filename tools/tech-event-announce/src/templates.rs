/// User-adjustable parts of the announcement texts.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Start time as written in the text, e.g. "14 Uhr".
    pub time: String,
    /// Name under the e-mail.
    pub signature: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            time: "14 Uhr".into(),
            signature: "sportfloh".into(),
        }
    }
}

pub fn chat(date: &str, topic: &str, description: &str, s: &Settings) -> String {
    format!(
        "Kommenden Samstag ({date}) ist wieder Tech-Event, zum Thema: {topic}\n\n\
         {description}\n\n\
         Wir starten wie immer um {time}; Eintritt ist wie immer kostenlos und ohne Anmeldung möglich.\n\
         Diese Info dürft Ihr gerne weiterleiten.",
        time = s.time
    )
}

pub fn email_subject(date: &str, topic: &str, s: &Settings) -> String {
    format!("Tech-Event - {topic} - Samstag {date} - {}", s.time)
}

pub fn email_body(date: &str, topic: &str, description: &str, s: &Settings) -> String {
    format!(
        "Hallo Zusammen,\n\n\
         Kommenden Samstag ({date}) ist wieder Tech-Event, zum Thema: {topic}\n\n\
         {description}\n\n\
         Wir starten wie immer um {time}; Eintritt ist wie immer kostenlos und ohne Anmeldung möglich.\n\
         Diese Info dürft Ihr gerne weiterleiten.\n\n\
         Gruß,\n\
         {signature}",
        time = s.time,
        signature = s.signature
    )
}

pub fn mastodon(date: &str, topic: &str, description: &str, s: &Settings) -> String {
    format!(
        "Kommenden Samstag ({date} ab {time}) ist wieder Tech-Event, zum Thema: {topic}\n\n\
         {description}",
        time = s.time
    )
}

/// Counts extended grapheme clusters (UAX #29), i.e. what a human perceives
/// as one "character" — unlike `.chars().count()`, a decomposed diacritic
/// (base + combining mark) or an emoji with a skin-tone/ZWJ modifier counts
/// as 1, not 2+.
pub fn grapheme_count(text: &str) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true).count()
}

/// Effective Mastodon character count: any `http://` or `https://` URL
/// (terminated by whitespace or end-of-string) counts as a flat 23
/// characters, mirroring Mastodon's own counting behavior, regardless of
/// its real length. Everything else is counted by grapheme cluster via
/// `grapheme_count`.
pub fn mastodon_char_count(text: &str) -> usize {
    let mut total = grapheme_count(text) as i64;
    let mut search_from = 0usize;

    while let Some(start) = find_url_start(&text[search_from..]).map(|p| p + search_from) {
        let rest = &text[start..];
        let url_byte_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let url = &rest[..url_byte_len];
        total += 23 - grapheme_count(url) as i64;
        search_from = start + url_byte_len;
    }

    total.max(0) as usize
}

fn find_url_start(s: &str) -> Option<usize> {
    let http = s.find("http://");
    let https = s.find("https://");
    match (http, https) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// Parse a German date `D.M.YYYY` / `DD.MM.YYYY` into `(year, month, day)`,
/// rejecting days that do not exist in that month (leap years included).
pub fn parse_de_date(s: &str) -> Option<(i32, u32, u32)> {
    let mut parts = s.trim().split('.');
    let (d, m, y) = (parts.next()?, parts.next()?, parts.next()?);
    let digits =
        |p: &str, lens: &[usize]| lens.contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit());
    if parts.next().is_some() || !digits(d, &[1, 2]) || !digits(m, &[1, 2]) || !digits(y, &[4]) {
        return None;
    }
    let (d, m, y): (u32, u32, i32) = (d.parse().ok()?, m.parse().ok()?, y.parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in_month = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (1..=days_in_month).contains(&d).then_some((y, m, d))
}

/// Day of the week for a Gregorian date, 0 = Sunday … 6 = Saturday.
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    // Sakamoto's algorithm.
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    (y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d as i32).rem_euclid(7) as u32
}

/// Hint shown under the date field: `None` for an empty field or a valid
/// Saturday, otherwise what is wrong with it.
pub fn date_warning(s: &str) -> Option<String> {
    const DAYS: [&str; 7] = [
        "Sonntag",
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
    ];
    if s.trim().is_empty() {
        return None;
    }
    let Some((y, m, d)) = parse_de_date(s) else {
        return Some("Ungültiges Datum (TT.MM.JJJJ)".into());
    };
    match weekday(y, m, d) {
        6 => None,
        wd => Some(format!("Achtung: kein Samstag ({})", DAYS[wd as usize])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- settings ---

    fn custom() -> Settings {
        Settings {
            time: "15:30 Uhr".into(),
            signature: "Das Orga-Team".into(),
        }
    }

    #[test]
    fn subject_uses_custom_time() {
        assert_eq!(
            email_subject("08.11.2025", "Rust", &custom()),
            "Tech-Event - Rust - Samstag 08.11.2025 - 15:30 Uhr"
        );
    }

    #[test]
    fn email_body_uses_custom_signature_and_time() {
        let r = email_body("08.11.2025", "Rust", "Text.", &custom());
        assert!(r.ends_with("Gruß,\nDas Orga-Team"), "got: {r}");
        assert!(r.contains("um 15:30 Uhr;"));
        assert!(!r.contains("14 Uhr") && !r.contains("sportfloh"));
    }

    #[test]
    fn chat_and_mastodon_use_custom_time() {
        assert!(chat("08.11.2025", "Rust", "Text.", &custom()).contains("um 15:30 Uhr;"));
        assert!(mastodon("08.11.2025", "Rust", "Text.", &custom()).contains("ab 15:30 Uhr)"));
    }

    // --- date validation ---

    #[test]
    fn parse_de_date_valid() {
        assert_eq!(parse_de_date("08.11.2025"), Some((2025, 11, 8)));
        assert_eq!(parse_de_date(" 8.11.2025 "), Some((2025, 11, 8)));
    }

    #[test]
    fn parse_de_date_rejects_impossible_dates() {
        assert_eq!(parse_de_date("31.04.2026"), None);
        assert_eq!(parse_de_date("00.01.2026"), None);
        assert_eq!(parse_de_date("12.13.2026"), None);
        assert_eq!(parse_de_date("2026-10-10"), None);
        assert_eq!(parse_de_date("10.10.26"), None);
        assert_eq!(parse_de_date(""), None);
    }

    #[test]
    fn parse_de_date_handles_leap_years() {
        assert_eq!(parse_de_date("29.02.2024"), Some((2024, 2, 29)));
        assert_eq!(parse_de_date("29.02.2025"), None);
        assert_eq!(parse_de_date("29.02.2000"), Some((2000, 2, 29)));
        assert_eq!(parse_de_date("29.02.1900"), None);
    }

    #[test]
    fn weekday_known_dates() {
        assert_eq!(weekday(2025, 11, 8), 6); // Saturday
        assert_eq!(weekday(2026, 10, 10), 6); // Saturday
        assert_eq!(weekday(2025, 11, 11), 2); // Tuesday
        assert_eq!(weekday(2024, 2, 29), 4); // Thursday
        assert_eq!(weekday(2000, 1, 1), 6); // Saturday
    }

    #[test]
    fn date_warning_messages() {
        assert_eq!(date_warning(""), None);
        assert_eq!(date_warning("10.10.2026"), None);
        assert_eq!(
            date_warning("11.11.2025").as_deref(),
            Some("Achtung: kein Samstag (Dienstag)")
        );
        assert_eq!(
            date_warning("31.04.2026").as_deref(),
            Some("Ungültiges Datum (TT.MM.JJJJ)")
        );
    }

    // --- chat ---

    #[test]
    fn chat_renders_full_template() {
        let r = chat(
            "08.11.2025",
            "Rust im Alltag",
            "Ein Vortrag über Rust.",
            &Settings::default(),
        );
        assert!(r.contains("Samstag (08.11.2025)"), "missing date in parens");
        assert!(r.contains("zum Thema: Rust im Alltag"), "missing topic");
        assert!(
            r.contains("Ein Vortrag über Rust.\n\nWir"),
            "description must be followed by blank line"
        );
        assert!(r.contains("14 Uhr"), "missing time");
        assert!(r.contains("kostenlos"), "missing free-entry line");
        assert!(r.contains("weiterleiten"), "missing forward-info line");
    }

    #[test]
    fn chat_with_empty_inputs_preserves_structure() {
        let r = chat("", "", "", &Settings::default());
        assert!(
            r.contains("Samstag ()"),
            "date slot should be empty inside parens"
        );
        assert!(r.contains("zum Thema: "), "topic slot should be empty");
        assert!(r.contains("14 Uhr"));
    }

    // --- email_subject ---

    #[test]
    fn email_subject_renders_correctly() {
        assert_eq!(
            email_subject("08.11.2025", "Rust im Alltag", &Settings::default()),
            "Tech-Event - Rust im Alltag - Samstag 08.11.2025 - 14 Uhr"
        );
    }

    #[test]
    fn email_subject_empty_inputs() {
        assert_eq!(
            email_subject("", "", &Settings::default()),
            "Tech-Event -  - Samstag  - 14 Uhr"
        );
    }

    // --- email_body ---

    #[test]
    fn email_body_renders_full_template() {
        let r = email_body(
            "08.11.2025",
            "Rust im Alltag",
            "Ein Vortrag über Rust.",
            &Settings::default(),
        );
        assert!(r.starts_with("Hallo Zusammen,"), "must start with greeting");
        assert!(r.contains("Samstag (08.11.2025)"));
        assert!(r.contains("zum Thema: Rust im Alltag"));
        assert!(
            r.contains("Ein Vortrag über Rust.\n\nWir"),
            "description must be followed by blank line"
        );
        assert!(r.contains("kostenlos"));
        assert!(r.contains("Gruß,"), "missing sign-off");
        assert!(r.contains("sportfloh"), "missing name");
    }

    // --- mastodon ---

    #[test]
    fn mastodon_renders_correctly() {
        let r = mastodon(
            "08.11.2025",
            "Rust im Alltag",
            "Ein Vortrag.",
            &Settings::default(),
        );
        assert!(r.contains("08.11.2025 ab 14 Uhr"), "missing date+time");
        assert!(r.contains("zum Thema: Rust im Alltag"));
        assert!(r.contains("Ein Vortrag."));
    }

    // --- grapheme_count ---

    #[test]
    fn grapheme_count_treats_combining_diacritic_as_one() {
        assert_eq!(grapheme_count("a\u{0308}"), 1);
    }

    // --- mastodon_char_count ---

    #[test]
    fn mastodon_char_count_plain_text_matches_naive_count() {
        let s = "Kommenden Samstag ist wieder Tech-Event, ohne Links heute.";
        assert_eq!(mastodon_char_count(s), 58);
    }

    #[test]
    fn mastodon_char_count_single_url_counts_as_23() {
        assert_eq!(mastodon_char_count("Hello https://example.com world"), 35);
    }

    #[test]
    fn mastodon_char_count_multiple_urls_each_counts_as_23() {
        assert_eq!(
            mastodon_char_count("See https://a.com and http://b.com"),
            55
        );
    }

    #[test]
    fn mastodon_char_count_url_at_start_of_text() {
        assert_eq!(mastodon_char_count("https://a.com is great"), 32);
    }

    #[test]
    fn mastodon_char_count_url_at_end_of_text_no_trailing_space() {
        assert_eq!(
            mastodon_char_count("Check this out: https://a.com/page"),
            39
        );
    }

    #[test]
    fn mastodon_char_count_url_glued_to_trailing_punctuation_included_in_span() {
        assert_eq!(mastodon_char_count("Link: https://a.com/x, see more"), 38);
    }

    #[test]
    fn mastodon_char_count_long_url_still_counts_as_23() {
        let s = format!("Anmeldung hier: https://{} vielen Dank", "a".repeat(50));
        assert_eq!(mastodon_char_count(&s), 51);
    }

    #[test]
    fn mastodon_char_count_no_url_returns_plain_chars_count() {
        let s = "Über Rust und Größe – ohne Link.";
        assert_eq!(mastodon_char_count(s), grapheme_count(s));
    }

    #[test]
    fn mastodon_char_count_combining_diacritic_counts_as_one_grapheme() {
        assert_eq!(mastodon_char_count("a\u{0308}bc"), 3);
    }

    #[test]
    fn mastodon_char_count_emoji_with_modifier_counts_as_one_grapheme() {
        assert_eq!(mastodon_char_count("\u{1F44D}\u{1F3FD} toll"), 6);
    }
}
