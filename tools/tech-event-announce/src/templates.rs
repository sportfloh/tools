pub fn chat(date: &str, topic: &str, description: &str) -> String {
    format!(
        "Kommenden Samstag ({date}) ist wieder Tech-Event, zum Thema: {topic}\n\n\
         {description}\n\n\
         Wir starten wie immer um 14 Uhr; Eintritt ist wie immer kostenlos und ohne Anmeldung möglich.\n\
         Diese Info dürft Ihr gerne weiterleiten."
    )
}

pub fn email_subject(date: &str, topic: &str) -> String {
    format!("Tech-Event - {topic} - Samstag {date} - 14 Uhr")
}

pub fn email_body(date: &str, topic: &str, description: &str) -> String {
    format!(
        "Hallo Zusammen,\n\n\
         Kommenden Samstag ({date}) ist wieder Tech-Event, zum Thema: {topic}\n\n\
         {description}\n\n\
         Wir starten wie immer um 14 Uhr; Eintritt ist wie immer kostenlos und ohne Anmeldung möglich.\n\
         Diese Info dürft Ihr gerne weiterleiten.\n\n\
         Gruß,\n\
         sportfloh"
    )
}

pub fn mastodon(date: &str, topic: &str, description: &str) -> String {
    format!(
        "Kommenden Samstag ({date} ab 14 Uhr) ist wieder Tech-Event, zum Thema: {topic}\n\n\
         {description}"
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- chat ---

    #[test]
    fn chat_renders_full_template() {
        let r = chat("08.11.2025", "Rust im Alltag", "Ein Vortrag über Rust.");
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
        let r = chat("", "", "");
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
            email_subject("08.11.2025", "Rust im Alltag"),
            "Tech-Event - Rust im Alltag - Samstag 08.11.2025 - 14 Uhr"
        );
    }

    #[test]
    fn email_subject_empty_inputs() {
        assert_eq!(email_subject("", ""), "Tech-Event -  - Samstag  - 14 Uhr");
    }

    // --- email_body ---

    #[test]
    fn email_body_renders_full_template() {
        let r = email_body("08.11.2025", "Rust im Alltag", "Ein Vortrag über Rust.");
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
        let r = mastodon("08.11.2025", "Rust im Alltag", "Ein Vortrag.");
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
