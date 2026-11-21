//! The installable app: registering the offline cache (sw.js), taking new
//! versions only at safe moments, and the browser's install prompt.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::ServiceWorkerRegistration;

/// A new version waiting is taken when the app comes back after this long hidden.
const HIDDEN_LONG_MS: f64 = 30.0 * 60.0 * 1000.0;
/// Guards against reloading in a loop when taking a new version.
const RELOADED_KEY: &str = "garden-sw-reloaded";

/// The browser's saved install prompt, when it offered one.
#[derive(Clone, Copy)]
pub struct InstallPrompt(pub RwSignal<Option<JsValue>, LocalStorage>);

/// The offline cache's registration, once it exists.
#[derive(Clone, Copy)]
pub struct Worker(pub StoredValue<Option<ServiceWorkerRegistration>, LocalStorage>);

pub fn provide_pwa() {
    let prompt = InstallPrompt(RwSignal::new_local(None));
    provide_context(prompt);
    let worker = Worker(StoredValue::new_local(None));
    provide_context(worker);
    listen_for_install(prompt);
    if cfg!(not(debug_assertions)) {
        leptos::task::spawn_local(register(worker));
    }
}

fn listen_for_install(prompt: InstallPrompt) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_prompt = Closure::<dyn Fn(web_sys::Event)>::new(move |e: web_sys::Event| {
        // keep it for our own button instead of the browser's mini bar
        e.prevent_default();
        prompt.0.set(Some(e.into()));
    });
    let _ = window.add_event_listener_with_callback(
        "beforeinstallprompt",
        on_prompt.as_ref().unchecked_ref(),
    );
    on_prompt.forget();
    let on_installed = Closure::<dyn Fn()>::new(move || prompt.0.set(None));
    let _ = window
        .add_event_listener_with_callback("appinstalled", on_installed.as_ref().unchecked_ref());
    on_installed.forget();
}

/// Show the browser's install prompt saved earlier.
pub fn install(prompt: InstallPrompt) {
    let Some(event) = prompt.0.get_untracked() else {
        return;
    };
    if let Ok(f) = js_sys::Reflect::get(&event, &"prompt".into())
        && let Ok(f) = f.dyn_into::<js_sys::Function>()
    {
        let _ = f.call0(&event);
    }
    prompt.0.set(None);
}

fn session() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.session_storage().ok().flatten())
}

/// Ask a waiting version to take over, then reload once it has.
fn take_waiting(reg: &ServiceWorkerRegistration) -> bool {
    let Some(waiting) = reg.waiting() else {
        return false;
    };
    if session()
        .and_then(|s| s.get_item(RELOADED_KEY).ok().flatten())
        .is_some()
    {
        return false; // already reloaded for this in this tab
    }
    let msg = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&msg, &"type".into(), &"skip-waiting".into());
    let _ = waiting.post_message(&msg);
    true
}

async fn register(worker: Worker) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let container = window.navigator().service_worker();
    let Ok(reg) = JsFuture::from(container.register("./sw.js")).await else {
        return;
    };
    let Ok(reg) = reg.dyn_into::<ServiceWorkerRegistration>() else {
        return;
    };
    worker.0.set_value(Some(reg.clone()));

    // reload once the new version controls the page (only when we asked)
    let asked = Rc::new(Cell::new(false));
    {
        let asked = asked.clone();
        let on_change = Closure::<dyn Fn()>::new(move || {
            if asked.get() {
                if let Some(s) = session() {
                    let _ = s.set_item(RELOADED_KEY, "1");
                }
                if let Some(w) = web_sys::window() {
                    let _ = w.location().reload();
                }
            }
        });
        let _ = container.add_event_listener_with_callback(
            "controllerchange",
            on_change.as_ref().unchecked_ref(),
        );
        on_change.forget();
    }

    // at start: a version that finished downloading last time takes over now
    if take_waiting(&reg) {
        asked.set(true);
    } else if let Some(s) = session() {
        let _ = s.remove_item(RELOADED_KEY);
    }

    // back after a long time hidden: check for and take a new version
    let hidden_at = Rc::new(Cell::new(None::<f64>));
    let on_visibility = Closure::<dyn Fn()>::new(move || {
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };
        let now = js_sys::Date::now();
        if doc.visibility_state() == web_sys::VisibilityState::Hidden {
            hidden_at.set(Some(now));
            return;
        }
        let long = hidden_at
            .take()
            .or_else(test_hidden_since)
            .is_some_and(|t| now - t >= HIDDEN_LONG_MS);
        if !long {
            return;
        }
        let reg = reg.clone();
        let asked = asked.clone();
        leptos::task::spawn_local(async move {
            if let Ok(p) = reg.update() {
                let _ = JsFuture::from(p).await;
            }
            // update() returns while a new version may still be installing
            wait_installed(&reg).await;
            if let Some(s) = session() {
                let _ = s.remove_item(RELOADED_KEY);
            }
            if take_waiting(&reg) {
                asked.set(true);
            }
        });
    });
    if let Some(doc) = window.document() {
        let _ = doc.add_event_listener_with_callback(
            "visibilitychange",
            on_visibility.as_ref().unchecked_ref(),
        );
    }
    on_visibility.forget();
}

/// Wait until a version being installed has finished installing (or failed).
async fn wait_installed(reg: &ServiceWorkerRegistration) {
    let Some(installing) = reg.installing() else {
        return;
    };
    if installing.state() != web_sys::ServiceWorkerState::Installing {
        return;
    }
    let done = js_sys::Promise::new(&mut |resolve, _| {
        let worker = installing.clone();
        let on_state = Closure::<dyn Fn()>::new(move || {
            let state = worker.state();
            if state != web_sys::ServiceWorkerState::Installing {
                let _ = resolve.call0(&JsValue::NULL);
            }
        });
        installing.set_onstatechange(Some(on_state.as_ref().unchecked_ref()));
        on_state.forget();
    });
    let _ = JsFuture::from(done).await;
}

/// For browser tests: `sessionStorage["garden-hidden-since"]` stands in for a
/// long time hidden.
fn test_hidden_since() -> Option<f64> {
    let s = session()?;
    let v = s.get_item("garden-hidden-since").ok().flatten()?;
    let _ = s.remove_item("garden-hidden-since");
    v.parse().ok()
}
