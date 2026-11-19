//! Tracing over a photo or survey plan: add the image, show it under the plan,
//! set its scale, adjust opacity, remove it.

use std::collections::HashMap;

use garden_core::geometry::Point;
use garden_core::model::BackgroundImage;
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use garden_editor::tools::ToolKind;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, HtmlImageElement, Url};

use crate::app::now;
use crate::library::Library;

/// Largest image we accept.
pub const MAX_PHOTO_BYTES: f64 = 8.0 * 1024.0 * 1024.0;

/// Object URLs for background images, by image_ref.
#[derive(Clone, Copy)]
pub struct ImageCache(pub RwSignal<HashMap<String, String>>);

/// Whether the trace card is open, and its last message.
#[derive(Clone, Copy)]
pub struct Trace {
    pub open: RwSignal<bool>,
    pub message: RwSignal<Option<&'static str>>,
    /// Image refs already looked up (found or missing), so each is fetched and
    /// reported at most once.
    looked_up: StoredValue<std::collections::HashSet<String>, LocalStorage>,
}

pub fn provide_trace() {
    provide_context(ImageCache(RwSignal::new(HashMap::new())));
    provide_context(Trace {
        open: RwSignal::new(false),
        message: RwSignal::new(None),
        looked_up: StoredValue::new_local(std::collections::HashSet::new()),
    });
}

/// Natural pixel size of an image blob, or None if it isn't a readable image.
async fn image_size(blob: &Blob) -> Option<(u32, u32)> {
    let url = Url::create_object_url_with_blob(blob).ok()?;
    let img = HtmlImageElement::new().ok()?;
    let loaded = js_sys::Promise::new(&mut |resolve, reject| {
        img.set_onload(Some(&resolve));
        img.set_onerror(Some(&reject));
    });
    img.set_src(&url);
    let ok = JsFuture::from(loaded).await.is_ok();
    let _ = Url::revoke_object_url(&url);
    (ok && img.natural_width() > 0).then(|| (img.natural_width(), img.natural_height()))
}

/// Put `blob` under the plan as the photo to trace, then start "Set scale".
pub async fn add_photo(
    lib: Library,
    state: RwSignal<EditorState>,
    cache: ImageCache,
    trace: Trace,
    blob: Blob,
) {
    let mime = blob.type_();
    if blob.size() > MAX_PHOTO_BYTES {
        trace.message.set(Some(EN.photo_too_big));
        return;
    }
    if mime != "image/png" && mime != "image/jpeg" {
        trace.message.set(Some(EN.photo_failed));
        return;
    }
    let Some((w, h)) = image_size(&blob).await else {
        trace.message.set(Some(EN.photo_failed));
        return;
    };
    let image_ref = format!("bg-{}", uuid::Uuid::new_v4());
    match lib.store.get_value() {
        Some(store) => {
            if store.put_image(&image_ref, &blob).await.is_err() {
                // don't show a photo that would vanish on reload
                trace.message.set(Some(EN.photo_not_saved));
                trace.open.set(true);
                return;
            }
        }
        // no storage at all: the photo works for this session only
        None => trace.message.set(Some(EN.photo_not_saved)),
    }
    trace.looked_up.update_value(|s| {
        s.insert(image_ref.clone());
    });
    if let Ok(url) = Url::create_object_url_with_blob(&blob) {
        cache.0.update(|c| {
            c.insert(image_ref.clone(), url);
        });
    }
    // Start roughly filling the view; "Set scale" makes it exact.
    let (origin, mm_per_px) = state.with_untracked(|s| {
        let v = &s.viewport;
        let visible = v.px_to_mm(v.width);
        (
            v.screen_to_world(Point::new(v.width * 0.1, v.height * 0.1)),
            visible * 0.8 / f64::from(w.max(1)),
        )
    });
    let bg = BackgroundImage {
        image_ref,
        mime,
        width_px: w,
        height_px: h,
        origin,
        mm_per_px,
        rotation: 0.0,
        opacity: 0.6,
        locked: true,
    };
    if lib.store.with_value(Option::is_some) {
        trace.message.set(None);
    }
    state.update(|s| {
        s.set_background(Some(bg), now());
        s.switch_tool(ToolKind::SetScale, now());
    });
    trace.open.set(true);
}

/// Make sure the current background's image is loaded into the cache.
pub async fn ensure_image(lib: Library, cache: ImageCache, trace: Trace, image_ref: String) {
    // each image is fetched (and a missing one reported) only once
    let first_time = trace
        .looked_up
        .try_update_value(|s| s.insert(image_ref.clone()))
        .unwrap_or(false);
    if !first_time || cache.0.with_untracked(|c| c.contains_key(&image_ref)) {
        return;
    }
    let Some(store) = lib.store.get_value() else {
        return;
    };
    match store.get_image(&image_ref).await {
        Ok(Some(blob)) => {
            if let Ok(url) = Url::create_object_url_with_blob(&blob) {
                cache.0.update(|c| {
                    c.insert(image_ref, url);
                });
            }
        }
        _ => {
            trace.message.set(Some(EN.photo_missing));
            trace.open.set(true); // say so where the photo controls are
        }
    }
}

/// The photo, drawn in world coordinates under everything else.
pub fn background_view(state: RwSignal<EditorState>, cache: ImageCache) -> impl IntoView {
    move || {
        let bg = state.with(|s| s.doc().background.clone())?;
        let url = cache.0.with(|c| c.get(&bg.image_ref).cloned())?;
        let (w, h) = bg.world_size();
        let rotate = format!(
            "rotate({} {} {})",
            bg.rotation.to_degrees(),
            bg.origin.x,
            bg.origin.y
        );
        Some(view! {
            <image href=url x=bg.origin.x y=bg.origin.y width=w height=h opacity=bg.opacity
                transform=rotate preserveAspectRatio="none" class="trace-photo" />
        })
    }
}

/// Pick an image file with the system dialog.
pub async fn pick_image() -> Option<Blob> {
    let doc = web_sys::window()?.document()?;
    let input: web_sys::HtmlInputElement = doc.create_element("input").ok()?.unchecked_into();
    input.set_type("file");
    input.set_accept("image/png,image/jpeg");
    let chosen = js_sys::Promise::new(&mut |resolve, _reject| {
        let target = input.clone();
        let on_change = wasm_bindgen::closure::Closure::once_into_js(move || {
            let file = target.files().and_then(|f| f.get(0));
            let _ = resolve.call1(
                &wasm_bindgen::JsValue::NULL,
                &file.map(wasm_bindgen::JsValue::from).unwrap_or_default(),
            );
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
    });
    input.click();
    JsFuture::from(chosen).await.ok()?.dyn_into::<Blob>().ok()
}

/// Card for the traced photo (opened from the controls card).
#[component]
pub fn TraceCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let lib = expect_context::<Library>();
    let cache = expect_context::<ImageCache>();
    let trace = expect_context::<Trace>();
    let bg = Memo::new(move |_| state.with(|s| s.doc().background.clone()));
    let add = move |_| {
        spawn_local(async move {
            if let Some(blob) = pick_image().await {
                add_photo(lib, state, cache, trace, blob).await;
            }
        })
    };
    view! {
        <Show when=move || trace.open.get()>
            <section class="card trace-card" aria-label=EN.tool_trace>
                <header class="trace-head">
                    <h2>{EN.tool_trace}</h2>
                    <button class="icon-btn" aria-label=EN.close on:click=move |_| trace.open.set(false)>
                        {crate::icons::icon(crate::icons::Icon::Close)}
                    </button>
                </header>
                {move || match bg.get() {
                    None => view! {
                        <p class="trace-note">{EN.no_photo}</p>
                        <button class="trace-primary" on:click=add>{EN.add_photo}</button>
                    }
                    .into_any(),
                    Some(b) => {
                        let opacity = (b.opacity * 100.0).round();
                        let b2 = b.clone();
                        view! {
                            <label class="trace-row">
                                <span>{EN.opacity}</span>
                                <input type="range" min="10" max="100" step="5" prop:value=opacity.to_string()
                                    on:change=move |e| {
                                        if let Ok(v) = event_target_value(&e).parse::<f32>() {
                                            let mut next = b2.clone();
                                            next.opacity = (v / 100.0).clamp(0.1, 1.0);
                                            state.update(|s| s.set_background(Some(next), now()));
                                        }
                                    } />
                            </label>
                            <button class="trace-primary" on:click=move |_| {
                                state.update(|s| s.switch_tool(ToolKind::SetScale, now()))
                            }>{EN.tool_set_scale}</button>
                            <div class="trace-actions">
                                <button on:click=add>{EN.add_photo}</button>
                                <button class="danger-text" on:click=move |_| {
                                    state.update(|s| s.set_background(None, now()))
                                }>{EN.remove_photo}</button>
                            </div>
                        }
                        .into_any()
                    }
                }}
                {move || trace.message.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
            </section>
        </Show>
    }
}
