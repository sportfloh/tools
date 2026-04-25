use leptos::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};

use crate::templates;

#[component]
pub fn App() -> impl IntoView {
    let date_raw = RwSignal::new(String::new()); // YYYY-MM-DD from <input type="date">
    let topic = RwSignal::new(String::new());
    let descr = RwSignal::new(String::new());

    // Formatted date (DD.MM.YYYY) derived from the raw ISO value.
    let date = Memo::new(move |_| templates::format_date(&date_raw.get()));

    let chat_text = Memo::new(move |_| templates::chat(&date.get(), &topic.get(), &descr.get()));
    let email_subj = Memo::new(move |_| templates::email_subject(&date.get(), &topic.get()));
    let email_body =
        Memo::new(move |_| templates::email_body(&date.get(), &topic.get(), &descr.get()));
    let mastodon_text =
        Memo::new(move |_| templates::mastodon(&date.get(), &topic.get(), &descr.get()));

    view! {
        <div class="app">
            <header class="app-header">
                <div class="header-bar">
                    <h1>"Tech-Event Announce"</h1>
                </div>
            </header>
            <main class="app-main">
                <section class="form-card">
                    <div class="form-field">
                        <label class="form-label" for="inp-date">"Datum"</label>
                        <input
                            id="inp-date"
                            type="date"
                            class="form-input"
                            prop:value=date_raw
                            on:input=move |ev| date_raw.set(event_target_value(&ev))
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
                            rows="4"
                            placeholder="Kurze Beschreibung des Themas…"
                            prop:value=descr
                            on:input=move |ev| descr.set(event_target_value(&ev))
                        />
                    </div>
                </section>

                <OutputCard title="Chat" text=chat_text rows="6"/>
                <OutputCard title="E-Mail Betreff" text=email_subj rows="2"/>
                <OutputCard title="E-Mail Body" text=email_body rows="11"/>
                <OutputCard title="Mastodon" text=mastodon_text rows="5"/>
            </main>
        </div>
    }
}

#[component]
fn OutputCard(title: &'static str, text: Memo<String>, rows: &'static str) -> impl IntoView {
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
                <span class="output-title">{title}</span>
                <button
                    class=move || if copied.get() { "btn-copy copied" } else { "btn-copy" }
                    on:click=on_copy
                >
                    {move || if copied.get() { "Kopiert!" } else { "Kopieren" }}
                </button>
            </div>
            <textarea
                class="output-text"
                readonly
                rows=rows
                prop:value=move || text.get()
            />
        </div>
    }
}
