use leptos::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::{storage, templates};

// localStorage keys for the draft (the date is not stored: it always
// defaults to the next Saturday, so a stale date cannot come back).
const KEY_TOPIC: &str = "tea.topic";
const KEY_DESCR: &str = "tea.descr";
const KEY_TIME: &str = "tea.time";
const KEY_SIGNATURE: &str = "tea.signature";

/// Returns the date of the next Saturday (from today) as `DD.MM.YYYY`.
/// If today is Saturday, returns next week's Saturday.
///
/// Built from the calendar date (day-of-month overflow is normalised by
/// `Date`), not by adding 24 h steps, which lands on the previous day when
/// the clocks go forward in between.
fn next_saturday_str() -> String {
    let now = js_sys::Date::new_0();
    let days_ahead = templates::days_until_next_saturday(now.get_day());
    let t = js_sys::Date::new_with_year_month_day(
        now.get_full_year(),
        now.get_month() as i32,
        now.get_date() as i32 + days_ahead as i32,
    );
    format!(
        "{:02}.{:02}.{:04}",
        t.get_date(),
        t.get_month() + 1,
        t.get_full_year()
    )
}

#[component]
pub fn App() -> impl IntoView {
    // Date is stored and displayed directly as DD.MM.YYYY — no conversion layer.
    let date = RwSignal::new(next_saturday_str());
    // Settings as typed (persisted); an emptied field falls back to the default.
    let time_input = RwSignal::new(storage::load(KEY_TIME).unwrap_or_default());
    let signature_input = RwSignal::new(storage::load(KEY_SIGNATURE).unwrap_or_default());
    Effect::new(move |_| storage::save(KEY_TIME, &time_input.get()));
    Effect::new(move |_| storage::save(KEY_SIGNATURE, &signature_input.get()));
    let settings = Memo::new(move |_| {
        let defaults = templates::Settings::default();
        let or_default = |v: String, d: String| {
            let v = v.trim();
            if v.is_empty() { d } else { v.to_string() }
        };
        templates::Settings {
            time: or_default(time_input.get(), defaults.time),
            signature: or_default(signature_input.get(), defaults.signature),
        }
    });
    let topic = RwSignal::new(storage::load(KEY_TOPIC).unwrap_or_default());
    let descr = RwSignal::new(storage::load(KEY_DESCR).unwrap_or_default());
    Effect::new(move |_| storage::save(KEY_TOPIC, &topic.get()));
    Effect::new(move |_| storage::save(KEY_DESCR, &descr.get()));
    let clear_draft = move |_| {
        topic.set(String::new());
        descr.set(String::new());
    };

    let chat_text = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        let de = descr.get();
        templates::chat(d.trim(), t.trim(), de.trim(), &settings.get())
    });
    let email_subj = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        templates::email_subject(d.trim(), t.trim(), &settings.get())
    });
    let email_body = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        let de = descr.get();
        templates::email_body(d.trim(), t.trim(), de.trim(), &settings.get())
    });
    let mastodon_text = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        let de = descr.get();
        templates::mastodon(d.trim(), t.trim(), de.trim(), &settings.get())
    });

    let mail_href = Memo::new(move |_| templates::mailto_url(&email_subj.get(), &email_body.get()));

    let inputs_complete = Memo::new(move |_| {
        !date.get().trim().is_empty()
            && !topic.get().trim().is_empty()
            && !descr.get().trim().is_empty()
    });

    view! {
        <div class="app">
            <header class="app-header">
                <div class="header-bar">
                    <h1>"Tech-Event Announce"</h1>
                </div>
            </header>
            <main class="app-main">
                // ── Left column: inputs ──────────────────────────────────────
                <section class="form-card">
                    <div class="form-field">
                        <label class="form-label" for="inp-date">"Datum"</label>
                        <input
                            id="inp-date"
                            type="text"
                            class="form-input"
                            placeholder="dd.mm.yyyy"
                            prop:value=date
                            on:input=move |ev| date.set(event_target_value(&ev))
                            aria-describedby="date-hint"
                        />
                        <p id="date-hint" class="form-hint" aria-live="polite">
                            {move || templates::date_warning(&date.get())}
                        </p>
                    </div>
                    <div class="form-field">
                        <label class="form-label" for="inp-topic">"Thema"</label>
                        <input
                            id="inp-topic"
                            type="text"
                            class="form-input"
                            placeholder="z. B. Rust im Alltag"
                            prop:value=topic
                            on:input=move |ev| topic.set(event_target_value(&ev))
                        />
                    </div>
                    <div class="form-field">
                        <label class="form-label" for="inp-descr">"Beschreibung"</label>
                        <textarea
                            id="inp-descr"
                            class="form-textarea"
                            placeholder="Kurze Beschreibung des Themas…"
                            prop:value=descr
                            on:input=move |ev| descr.set(event_target_value(&ev))
                        />
                    </div>
                    <details class="settings">
                        <summary>"Einstellungen"</summary>
                        <div class="form-field">
                            <label class="form-label" for="inp-time">"Uhrzeit"</label>
                            <input
                                id="inp-time"
                                type="text"
                                class="form-input"
                                placeholder=templates::Settings::default().time
                                prop:value=time_input
                                on:input=move |ev| time_input.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="form-field">
                            <label class="form-label" for="inp-signature">"Signatur (E-Mail)"</label>
                            <input
                                id="inp-signature"
                                type="text"
                                class="form-input"
                                placeholder=templates::Settings::default().signature
                                prop:value=signature_input
                                on:input=move |ev| signature_input.set(event_target_value(&ev))
                            />
                        </div>
                    </details>
                    <div class="form-actions">
                        <button
                            class="btn-clear"
                            type="button"
                            on:click=clear_draft
                            disabled=move || topic.with(String::is_empty) && descr.with(String::is_empty)
                        >
                            "Leeren"
                        </button>
                    </div>
                </section>

                // ── Right column: outputs ────────────────────────────────────
                <div class="outputs-col">
                    <OutputCard title="Chat" text=chat_text enabled=inputs_complete shareable=true/>
                    <OutputCard title="E-Mail-Betreff" text=email_subj enabled=inputs_complete/>
                    <OutputCard
                        title="E-Mail-Text"
                        text=email_body
                        enabled=inputs_complete
                        mail_href=mail_href
                    />
                    <OutputCard
                        title="Mastodon"
                        text=mastodon_text
                        enabled=inputs_complete
                        char_limit=500_u32
                        char_count=templates::mastodon_char_count
                        shareable=true
                    />
                </div>
            </main>
        </div>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum CopyState {
    Idle,
    Copied,
    Failed,
}

/// Write `text` to the clipboard; `false` if the browser refused or has no
/// Clipboard API (it is `undefined` outside secure contexts).
async fn write_clipboard(text: &str) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let nav = window.navigator();
    let has_clipboard = js_sys::Reflect::get(&nav, &JsValue::from_str("clipboard"))
        .is_ok_and(|v| !v.is_undefined());
    has_clipboard
        && JsFuture::from(nav.clipboard().write_text(text))
            .await
            .is_ok()
}

/// Whether the browser offers the Web Share API (`navigator.share`).
fn share_supported() -> bool {
    web_sys::window().is_some_and(|w| {
        js_sys::Reflect::get(&w.navigator(), &JsValue::from_str("share"))
            .is_ok_and(|f| f.is_function())
    })
}

#[component]
fn OutputCard(
    title: &'static str,
    text: Memo<String>,
    enabled: Memo<bool>,
    #[prop(optional)] char_limit: Option<u32>,
    #[prop(optional)] char_count: Option<fn(&str) -> usize>,
    /// Adds an "E-Mail öffnen" link with this `mailto:` URL.
    #[prop(optional)]
    mail_href: Option<Memo<String>>,
    /// Adds a "Teilen" button (Web Share API) where the browser supports it.
    #[prop(optional)]
    shareable: bool,
) -> impl IntoView {
    let can_share = shareable && share_supported();
    let on_share = move |_| {
        let t = text.get_untracked();
        spawn_local(async move {
            if let Some(window) = web_sys::window() {
                let data = web_sys::ShareData::new();
                data.set_text(&t);
                // Rejects when the user cancels the share sheet; nothing to do.
                let _ = JsFuture::from(window.navigator().share_with_data(&data)).await;
            }
        });
    };
    let copy_state = RwSignal::new(CopyState::Idle);
    let count_fn = char_count.unwrap_or(templates::grapheme_count);

    let on_copy = move |_| {
        let t = text.get_untracked();
        spawn_local(async move {
            copy_state.set(if write_clipboard(&t).await {
                CopyState::Copied
            } else {
                CopyState::Failed
            });
            // Reset the label after 1.5 s using a setTimeout Promise.
            if let Some(window) = web_sys::window() {
                let promise = js_sys::Promise::new(&mut |resolve, _| {
                    let _ = window
                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 1500);
                });
                let _ = JsFuture::from(promise).await;
            }
            copy_state.set(CopyState::Idle);
        });
    };

    view! {
        <div class="output-card">
            <div class="output-header">
                <div class="output-title-group">
                    <span class="output-title">{title}</span>
                    {char_limit.map(|limit| view! {
                        <span class=move || {
                            if count_fn(&text.get()) > limit as usize {
                                "char-count over-limit"
                            } else {
                                "char-count"
                            }
                        }>
                            {move || format!("{}/{}", count_fn(&text.get()), limit)}
                        </span>
                    })}
                </div>
                <div class="output-actions">
                {mail_href.map(|href| view! {
                    <a
                        class="btn-copy btn-link"
                        class:disabled=move || !enabled.get()
                        href=move || enabled.get().then(|| href.get())
                        aria-disabled=move || (!enabled.get()).to_string()
                    >
                        "E-Mail öffnen"
                    </a>
                })}
                {can_share.then(|| view! {
                    <button
                        class="btn-copy"
                        type="button"
                        disabled=move || !enabled.get()
                        on:click=on_share
                    >
                        "Teilen"
                    </button>
                })}
                <button
                    class=move || match copy_state.get() {
                        CopyState::Idle => "btn-copy",
                        CopyState::Copied => "btn-copy copied",
                        CopyState::Failed => "btn-copy copy-failed",
                    }
                    disabled=move || !enabled.get()
                    on:click=on_copy
                >
                    {move || match copy_state.get() {
                        CopyState::Idle => "Kopieren",
                        CopyState::Copied => "Kopiert!",
                        CopyState::Failed => "Fehler",
                    }}
                </button>
                </div>
            </div>
            <textarea
                class="output-text"
                readonly
                prop:value=move || text.get()
            />
        </div>
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::next_saturday_str;
    use crate::templates::{parse_de_date, weekday};
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn next_saturday_is_a_saturday_in_the_future() {
        let s = next_saturday_str();
        let (y, m, d) = parse_de_date(&s).unwrap_or_else(|| panic!("not a date: {s}"));
        assert_eq!(weekday(y, m, d), 6, "{s} is not a Saturday");
        let date = js_sys::Date::new_with_year_month_day(y as u32, m as i32 - 1, d as i32);
        assert!(
            date.get_time() > js_sys::Date::now(),
            "{s} is not in the future"
        );
    }
}
