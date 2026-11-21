//! The full-screen 3D view. Its Bevy bundle (`loma3d/`) is downloaded the
//! first time it opens; each time it opens it gets a snapshot of the design.

use leptos::prelude::*;
use leptos::task::spawn_local;
use loma_editor::state::EditorState;
use loma_editor::strings::EN;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::icons::{Icon, icon};

/// Where the 3D bundle lives, relative to the page.
const MODULE_PATH: &str = "loma3d/loma_3d.js";
const CANVAS_ID: &str = "loma-3d";

#[wasm_bindgen(inline_js = "export function import_module(url) { return import(url); }")]
extern "C" {
    fn import_module(url: &str) -> js_sys::Promise;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Loading,
    Ready,
    Failed(&'static str),
}

/// Whether the 3D view is showing, and how loading went.
#[derive(Clone, Copy)]
pub struct View3d {
    pub open: RwSignal<bool>,
    status: RwSignal<Status>,
    /// The loaded 3D module (imported at most once).
    module: StoredValue<Option<JsValue>, LocalStorage>,
}

pub fn provide_view3d() {
    provide_context(View3d {
        open: RwSignal::new(false),
        status: RwSignal::new(Status::Idle),
        module: StoredValue::new_local(None),
    });
}

fn has_webgl2() -> bool {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return false;
    };
    let Ok(canvas) = doc.create_element("canvas") else {
        return false;
    };
    let canvas: web_sys::HtmlCanvasElement = canvas.unchecked_into();
    matches!(canvas.get_context("webgl2"), Ok(Some(_)))
}

/// Call `name(args…)` on the loaded module.
fn call(module: &JsValue, name: &str, args: &[&JsValue]) -> Result<JsValue, JsValue> {
    let f: js_sys::Function = js_sys::Reflect::get(module, &JsValue::from_str(name))?.dyn_into()?;
    let args: js_sys::Array = args.iter().copied().collect();
    f.apply(&JsValue::NULL, &args)
}

fn snapshot(state: RwSignal<EditorState>) -> Option<String> {
    state.with_untracked(|s| loma_core::io::to_json(s.doc()).ok())
}

/// Show the 3D view with the current design, loading the bundle the first time.
pub fn open_3d(view: View3d, state: RwSignal<EditorState>) {
    // a tab left on an older app version must not load a newer 3D bundle
    if crate::pwa::reload_if_stale(state) {
        return;
    }
    view.open.set(true);
    focus_canvas();
    if view.module.with_value(Option::is_some) && viewer_failed(view) {
        view.status.set(Status::Failed(EN.err_3d_crashed));
        return;
    }
    match view.status.get_untracked() {
        Status::Loading => {} // the design is read when loading finishes
        Status::Ready => {
            let module = view.module.get_value();
            if let (Some(module), Some(json)) = (module, snapshot(state))
                && call(&module, "show", &[&JsValue::from_str(&json)]).is_err()
            {
                view.status.set(Status::Failed(EN.err_3d_failed));
            }
        }
        Status::Idle | Status::Failed(_) => {
            if !has_webgl2() {
                view.status.set(Status::Failed(EN.err_no_webgl));
                return;
            }
            view.status.set(Status::Loading);
            spawn_local(async move {
                let result = load_and_start(view, state).await;
                match result {
                    Ok(()) => {
                        view.status.set(Status::Ready);
                        watch_for_crash(view).await;
                    }
                    Err(_) => view.status.set(Status::Failed(EN.err_3d_failed)),
                }
            });
        }
    }
}

/// Whether the loaded viewer reports that it crashed.
fn viewer_failed(view: View3d) -> bool {
    view.module
        .get_value()
        .and_then(|m| call(&m, "has_failed", &[]).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Wait `ms` milliseconds.
async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = JsFuture::from(p).await;
}

/// The viewer can fail after it starts (e.g. the GPU can't do what it
/// needs); check now and then so people get a message instead of a blank sky.
async fn watch_for_crash(view: View3d) {
    loop {
        sleep(1500).await;
        if viewer_failed(view) {
            view.status.set(Status::Failed(EN.err_3d_crashed));
            return;
        }
    }
}

/// Keyboard focus goes to the 3D view, so Space/Enter can't press the
/// button that opened it.
fn focus_canvas() {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(CANVAS_ID))
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = el.focus();
    }
}

async fn load_and_start(view: View3d, state: RwSignal<EditorState>) -> Result<(), JsValue> {
    let module = match view.module.get_value() {
        Some(m) => m,
        None => {
            let base = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.base_uri().ok().flatten())
                .unwrap_or_default();
            let url = web_sys::Url::new_with_base(MODULE_PATH, &base)?.href();
            let module = JsFuture::from(import_module(&url)).await?;
            // the bundle's default export fetches and starts its wasm
            JsFuture::from(js_sys::Promise::from(call(&module, "default", &[])?)).await?;
            view.module.set_value(Some(module.clone()));
            module
        }
    };
    let json = snapshot(state).ok_or_else(|| JsValue::from_str("design"))?;
    call(
        &module,
        "open",
        &[
            &JsValue::from_str(&format!("#{CANVAS_ID}")),
            &JsValue::from_str(&json),
        ],
    )?;
    Ok(())
}

/// The full-screen 3D view. Stays in the page once opened, hidden when closed,
/// so the viewer isn't restarted.
#[component]
pub fn View3dOverlay() -> impl IntoView {
    let view = expect_context::<View3d>();
    let close = move |_| view.open.set(false);
    // touch wording on phones and tablets (a finger is the main pointer)
    let touch = web_sys::window()
        .and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
        .is_some_and(|m| m.matches());
    let help = if touch { EN.help_3d_touch } else { EN.help_3d };
    let status = move || match view.status.get() {
        Status::Loading => Some(view! { <p class="view3d-status">{EN.loading_3d}</p> }.into_any()),
        Status::Failed(m) => {
            Some(view! { <p class="view3d-status error" role="alert">{m}</p> }.into_any())
        }
        Status::Idle | Status::Ready => None,
    };
    view! {
        <section class="view3d" class:shown=move || view.open.get() aria-label=EN.view_3d
            aria-hidden=move || (!view.open.get()).to_string()>
            <canvas id=CANVAS_ID tabindex="0"></canvas>
            <header class="view3d-bar">
                <h2>{EN.view_3d}</h2>
                <button class="view3d-close" on:click=close>{icon(Icon::Close)}{EN.close_3d}</button>
            </header>
            {status}
            <p class="view3d-help">{help}</p>
        </section>
    }
}
