use leptos::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::templates;

/// Returns the date of the next Saturday (from today) as `DD.MM.YYYY`.
/// If today is Saturday, returns next week's Saturday.
fn next_saturday_str() -> String {
    let now = js_sys::Date::new_0();
    let day = now.get_day() as i32; // 0 = Sunday … 6 = Saturday
    let days_ahead = {
        let d = (6 - day).rem_euclid(7);
        if d == 0 { 7 } else { d }
    };
    let target_ms = now.get_time() + days_ahead as f64 * 86_400_000.0;
    let t = js_sys::Date::new(&JsValue::from_f64(target_ms));
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
    let topic = RwSignal::new(String::new());
    let descr = RwSignal::new(String::new());

    let chat_text = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        let de = descr.get();
        templates::chat(d.trim(), t.trim(), de.trim())
    });
    let email_subj = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        templates::email_subject(d.trim(), t.trim())
    });
    let email_body = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        let de = descr.get();
        templates::email_body(d.trim(), t.trim(), de.trim())
    });
    let mastodon_text = Memo::new(move |_| {
        let d = date.get();
        let t = topic.get();
        let de = descr.get();
        templates::mastodon(d.trim(), t.trim(), de.trim())
    });

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
                </section>

                // ── Right column: outputs ────────────────────────────────────
                <div class="outputs-col">
                    <OutputCard title="Chat" text=chat_text enabled=inputs_complete/>
                    <OutputCard title="EmailBetreff" text=email_subj enabled=inputs_complete/>
                    <OutputCard title="EmailBody" text=email_body enabled=inputs_complete/>
                    <OutputCard
                        title="Mastodon"
                        text=mastodon_text
                        enabled=inputs_complete
                        char_limit=500_u32
                        char_count=templates::mastodon_char_count
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

#[component]
fn OutputCard(
    title: &'static str,
    text: Memo<String>,
    enabled: Memo<bool>,
    #[prop(optional)] char_limit: Option<u32>,
    #[prop(optional)] char_count: Option<fn(&str) -> usize>,
) -> impl IntoView {
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
            <textarea
                class="output-text"
                readonly
                prop:value=move || text.get()
            />
        </div>
    }
}
