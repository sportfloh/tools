//! Bottom-of-screen toasts (with optional Undo).

use leptos::prelude::*;

// ─── Toasts ───────────────────────────────────────────────────────────────────

const TOAST_DURATION: std::time::Duration = std::time::Duration::from_millis(4000);

/// A short message shown at the bottom of the screen, optionally with Undo.
/// The undo action is a plain `Arc<dyn Fn>` rather than a Leptos `Callback`,
/// which would be owned by (and disposed with) the scope that created it.
#[derive(Clone)]
pub(crate) struct Toast {
    pub message: String,
    pub undo: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

/// Copyable handle (provided via context) for showing toasts. A newer toast
/// replaces the current one; the generation counter keeps an older timer
/// from hiding it early.
#[derive(Clone, Copy)]
pub(crate) struct Toasts {
    current: RwSignal<Option<Toast>>,
    generation: StoredValue<u64>,
}

impl Toasts {
    pub(crate) fn new() -> Self {
        Toasts {
            current: RwSignal::new(None),
            generation: StoredValue::new(0),
        }
    }

    pub(crate) fn show(self, message: impl Into<String>) {
        self.push(Toast {
            message: message.into(),
            undo: None,
        });
    }

    pub(crate) fn show_with_undo(
        self,
        message: impl Into<String>,
        undo: impl Fn() + Send + Sync + 'static,
    ) {
        self.push(Toast {
            message: message.into(),
            undo: Some(std::sync::Arc::new(undo)),
        });
    }

    fn push(self, toast: Toast) {
        let generation = self.generation.get_value() + 1;
        self.generation.set_value(generation);
        self.current.set(Some(toast));
        set_timeout(
            move || {
                if self.generation.get_value() == generation {
                    self.current.set(None);
                }
            },
            TOAST_DURATION,
        );
    }

    fn dismiss(self) {
        self.current.set(None);
    }
}

#[component]
pub fn ToastBar() -> impl IntoView {
    let toasts = use_context::<Toasts>().expect("toasts context");
    let message = move || {
        toasts
            .current
            .with(|t| t.as_ref().map(|t| t.message.clone()).unwrap_or_default())
    };
    let undo = move || {
        toasts
            .current
            .with(|t| t.as_ref().and_then(|t| t.undo.clone()))
    };
    view! {
        // Always rendered so screen readers pick up changes in the live region.
        <div class="toast-region" role="status" aria-live="polite">
            <Show when=move || toasts.current.with(Option::is_some)>
                <div class="toast">
                    <span class="toast-message">{message}</span>
                    {move || undo().map(|undo| view! {
                        <button
                            class="toast-undo"
                            type="button"
                            on:click=move |_| {
                                toasts.dismiss();
                                undo();
                            }
                        >
                            "Undo"
                        </button>
                    })}
                </div>
            </Show>
        </div>
    }
}
