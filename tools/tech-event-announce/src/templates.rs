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
}
