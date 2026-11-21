//! The installable app: registering the offline cache (sw.js), taking new
//! versions only at safe moments, and the browser's install prompt.

use std::cell::Cell;
use std::rc::Rc;

use garden_editor::state::EditorState;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::ServiceWorkerRegistration;

use crate::library::{Library, leave_current};

/// A new version waiting is taken when the app comes back after this long hidden.
const HIDDEN_LONG_MS: f64 = 30.0 * 60.0 * 1000.0;
/// Guards against reloading in a loop when taking a new version.
const RELOADED_KEY: &str = "garden-sw-reloaded";

/// The browser's saved install prompt, when it offered one.
#[derive(Clone, Copy)]
pub struct InstallPrompt(pub RwSignal<Option<JsValue>, LocalStorage>);

/// The offline cache's registration, once it exists, and whether another tab
/// has switched to a newer version under this one.
#[derive(Clone, Copy)]
pub struct Worker {
    pub reg: StoredValue<Option<ServiceWorkerRegistration>, LocalStorage>,
    pub stale: StoredValue<bool, LocalStorage>,
}

/// What a safe reload needs: the design, to save it first.
#[derive(Clone, Copy)]
struct App {
    state: RwSignal<EditorState>,
    lib: Library,
    worker: Worker,
}

pub fn provide_pwa(state: RwSignal<EditorState>, lib: Library) {
    let prompt = InstallPrompt(RwSignal::new_local(None));
    provide_context(prompt);
    let worker = Worker {
        reg: StoredValue::new_local(None),
        stale: StoredValue::new_local(false),
    };
    provide_context(worker);
    listen_for_install(prompt);
    if cfg!(not(debug_assertions)) {
        leptos::task::spawn_local(register(App { state, lib, worker }));
    }
}

/// Save the design, then reload, unless a gesture is in progress or the
/// design couldn't be saved. Returns whether it reloaded.
async fn reload_safely(app: App) -> bool {
    if app.state.with_untracked(EditorState::is_busy) {
        return false;
    }
    if !leave_current(app.lib, app.state).await {
        return false;
    }
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
    true
}

/// Before opening 3D: a tab left on an older version (another tab switched)
/// reloads first, so old app code never loads a newer 3D bundle. Returns true
/// when it is reloading instead.
pub fn reload_if_stale(state: RwSignal<EditorState>) -> bool {
    let (Some(worker), Some(lib)) = (use_context::<Worker>(), use_context::<Library>()) else {
        return false;
    };
    if !worker.stale.get_value() {
        return false;
    }
    leptos::task::spawn_local(async move {
        reload_safely(App { state, lib, worker }).await;
    });
    true
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

/// Ask a waiting version to take over (then this tab saves and reloads), but
/// not in the middle of a gesture.
fn take_waiting(app: App, reg: &ServiceWorkerRegistration) -> bool {
    let Some(waiting) = reg.waiting() else {
        return false;
    };
    if app.state.with_untracked(EditorState::is_busy) {
        return false;
    }
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

async fn register(app: App) {
    let worker = app.worker;
    let Some(window) = web_sys::window() else {
        return;
    };
    let container = window.navigator().service_worker();
    // check sw.js *and* cache-list.js on the server, never the HTTP cache,
    // or a host's caching headers could hide a new version for hours
    let options = web_sys::RegistrationOptions::new();
    options.set_update_via_cache(web_sys::ServiceWorkerUpdateViaCache::None);
    let Ok(reg) = JsFuture::from(container.register_with_options("./sw.js", &options)).await else {
        return;
    };
    let Ok(reg) = reg.dyn_into::<ServiceWorkerRegistration>() else {
        return;
    };
    worker.reg.set_value(Some(reg.clone()));

    // when a new version takes control: the tab that asked saves and reloads;
    // any other tab is now stale and reloads at its next safe moment
    let asked = Rc::new(Cell::new(false));
    {
        let asked = asked.clone();
        let on_change = Closure::<dyn Fn()>::new(move || {
            if !asked.get() {
                worker.stale.set_value(true);
                return;
            }
            if let Some(s) = session() {
                let _ = s.set_item(RELOADED_KEY, "1");
            }
            leptos::task::spawn_local(async move {
                if !reload_safely(app).await {
                    worker.stale.set_value(true);
                }
            });
        });
        let _ = container.add_event_listener_with_callback(
            "controllerchange",
            on_change.as_ref().unchecked_ref(),
        );
        on_change.forget();
    }

    // at start: a version that finished downloading last time takes over now
    if take_waiting(app, &reg) {
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
        // another tab switched versions while this one was away
        if worker.stale.get_value() {
            leptos::task::spawn_local(async move {
                reload_safely(app).await;
            });
            return;
        }
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
            if take_waiting(app, &reg) {
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
