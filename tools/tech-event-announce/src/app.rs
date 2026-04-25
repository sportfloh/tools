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
                        />
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
                    <OutputCard title="Mastodon" text=mastodon_text enabled=inputs_complete char_limit=500_u32/>
                </div>
            </main>
        </div>
    }
}

#[component]
fn OutputCard(
    title: &'static str,
    text: Memo<String>,
    enabled: Memo<bool>,
    #[prop(optional)] char_limit: Option<u32>,
) -> impl IntoView {
    let copied = RwSignal::new(false);

    let on_copy = move |_| {
        let t = text.get_untracked();
        copied.set(true);
        spawn_local(async move {
            if let Some(window) = web_sys::window() {
                let clipboard = window.navigator().clipboard();
                let _ = JsFuture::from(clipboard.write_text(&t)).await;
                // Reset the "Kopiert!" label after 1.5 s using a setTimeout Promise.
                let promise = js_sys::Promise::new(&mut |resolve, _| {
                    let _ = window
                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 1500);
                });
                let _ = JsFuture::from(promise).await;
            }
            copied.set(false);
        });
    };

    view! {
        <div class="output-card">
            <div class="output-header">
                <div class="output-title-group">
                    <span class="output-title">{title}</span>
                    {char_limit.map(|limit| view! {
                        <span class=move || {
                            if text.get().chars().count() > limit as usize {
                                "char-count over-limit"
                            } else {
                                "char-count"
                            }
                        }>
                            {move || format!("{}/{}", text.get().chars().count(), limit)}
                        </span>
                    })}
                </div>
                <button
                    class=move || if copied.get() { "btn-copy copied" } else { "btn-copy" }
                    disabled=move || !enabled.get()
                    on:click=on_copy
                >
                    {move || if copied.get() { "Kopiert!" } else { "Kopieren" }}
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
