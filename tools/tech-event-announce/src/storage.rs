//! Tiny `localStorage` wrapper. Every access is fallible in the browser
//! (private mode, blocked site data), so failures just mean "nothing stored".

/// The stored value for `key`, if storage is available and holds one.
pub fn load(key: &str) -> Option<String> {
    local_storage()?.get_item(key).ok().flatten()
}

/// Store `value` under `key`; silently does nothing if storage is unavailable.
pub fn save(key: &str, value: &str) {
    if let Some(storage) = local_storage() {
        let _ = storage.set_item(key, value);
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::{load, save};
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn storage_round_trip() {
        save("tea.test", "Rust im Alltag\nzweite Zeile");
        assert_eq!(
            load("tea.test").as_deref(),
            Some("Rust im Alltag\nzweite Zeile")
        );
    }

    #[wasm_bindgen_test]
    fn storage_missing_key_is_none() {
        assert_eq!(load("tea.does-not-exist"), None);
    }
}
