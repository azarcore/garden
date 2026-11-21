//! Plant photos: scaled down to at most 1600 px, saved as JPEG in the
//! browser's image store, shown as dated thumbnails on the plant card.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use leptos::task::spawn_local;
use loma_core::model::{ObjectId, Photo};
use loma_editor::care::date_label;
use loma_editor::state::EditorState;
use loma_editor::strings::EN;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, HtmlCanvasElement, HtmlImageElement, HtmlInputElement, Url};

use crate::app::now;
use crate::garden::today;
use crate::library::Library;
use crate::storage::{Store, StoredDesign};

/// Longest side of a saved photo, in pixels.
const MAX_SIDE: u32 = 1600;
const JPEG_QUALITY: f64 = 0.85;
/// Unused photos younger than this are kept: another tab may be about to save
/// the design that uses them.
const KEEP_NEW_MS: i64 = 60 * 60 * 1000;

/// Object URLs of plant photos by image_ref; None when the photo isn't on this device.
#[derive(Clone, Copy)]
struct PhotoCache(RwSignal<HashMap<String, Option<String>>>);

fn cache() -> PhotoCache {
    expect_context::<PhotoCache>()
}

pub fn provide_photos() {
    provide_context(PhotoCache(RwSignal::new(HashMap::new())));
}

/// The size a photo is scaled to: at most `MAX_SIDE` on its long side, never enlarged.
fn scaled(w: u32, h: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= MAX_SIDE {
        return (w, h);
    }
    let f = f64::from(MAX_SIDE) / f64::from(long);
    (
        ((f64::from(w) * f).round() as u32).max(1),
        ((f64::from(h) * f).round() as u32).max(1),
    )
}

/// Decode `blob`, draw it at most 1600 px on its long side and encode as JPEG.
async fn shrink(blob: &Blob) -> Option<Blob> {
    let url = Url::create_object_url_with_blob(blob).ok()?;
    let img = HtmlImageElement::new().ok()?;
    let loaded = js_sys::Promise::new(&mut |resolve, reject| {
        img.set_onload(Some(&resolve));
        img.set_onerror(Some(&reject));
    });
    img.set_src(&url);
    let ok = JsFuture::from(loaded).await.is_ok();
    let _ = Url::revoke_object_url(&url);
    if !ok || img.natural_width() == 0 {
        return None;
    }
    let (w, h) = scaled(img.natural_width(), img.natural_height());
    let canvas: HtmlCanvasElement = web_sys::window()?
        .document()?
        .create_element("canvas")
        .ok()?
        .unchecked_into();
    canvas.set_width(w);
    canvas.set_height(h);
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok()??.unchecked_into();
    ctx.draw_image_with_html_image_element_and_dw_and_dh(&img, 0.0, 0.0, w.into(), h.into())
        .ok()?;
    let encoded = js_sys::Promise::new(&mut |resolve, _reject| {
        let done = Closure::once_into_js(move |b: JsValue| {
            let _ = resolve.call1(&JsValue::NULL, &b);
        });
        let _ = canvas.to_blob_with_type_and_encoder_options(
            done.unchecked_ref(),
            "image/jpeg",
            &JsValue::from_f64(JPEG_QUALITY),
        );
    });
    JsFuture::from(encoded).await.ok()?.dyn_into::<Blob>().ok()
}

/// Shrink, store and attach a photo to a plant.
async fn add(
    state: RwSignal<EditorState>,
    lib: Library,
    photos: PhotoCache,
    id: ObjectId,
    file: Blob,
    error: RwSignal<Option<&'static str>>,
) {
    if file.size() > 40.0 * 1024.0 * 1024.0 {
        return error.set(Some(EN.plant_photo_too_big));
    }
    let Some(small) = shrink(&file).await else {
        return error.set(Some(EN.photo_failed));
    };
    let Some(store) = lib.store.get_value() else {
        return error.set(Some(EN.photo_not_saved));
    };
    let image_ref = format!("photo-{}-{}", now(), uuid::Uuid::new_v4());
    if store.put_image(&image_ref, &small).await.is_err() {
        return error.set(Some(EN.photo_not_saved));
    }
    if let Ok(url) = Url::create_object_url_with_blob(&small) {
        photos.0.update(|c| {
            c.insert(image_ref.clone(), Some(url));
        });
    }
    error.set(None);
    state.update(|s| {
        s.add_photo(
            id,
            Photo {
                image_ref,
                date: today(),
                added_ms: now(),
            },
            now(),
        )
    });
}

/// Look a photo up in storage once.
fn ensure(lib: Library, image_ref: String) {
    let c = cache();
    if c.0.with_untracked(|m| m.contains_key(&image_ref)) {
        return;
    }
    c.0.update(|m| {
        m.insert(image_ref.clone(), None);
    });
    spawn_local(async move {
        let Some(store) = lib.store.get_value() else {
            return;
        };
        if let Ok(Some(blob)) = store.get_image(&image_ref).await
            && let Ok(url) = Url::create_object_url_with_blob(&blob)
        {
            c.0.update(|m| {
                m.insert(image_ref, Some(url));
            });
        }
    });
}

fn image_view(key: String, alt: String, class: &'static str) -> impl IntoView {
    let c = cache();
    move || match c.0.with(|m| m.get(&key).cloned().flatten()) {
        Some(url) => view! { <img class=class src=url alt=alt.clone() /> }.into_any(),
        None => {
            view! { <span class=format!("{class} photo-missing")>{EN.plant_photo_missing}</span> }
                .into_any()
        }
    }
}

/// The photos section of a plant card.
pub fn photo_strip(state: RwSignal<EditorState>, id: ObjectId) -> impl IntoView {
    let lib = expect_context::<Library>();
    let photo_cache = cache();
    let error = RwSignal::new(None::<&'static str>);
    let viewing = RwSignal::new(None::<Photo>);
    let mut photos = state.with_untracked(|s| s.care_of(id).photos);
    photos.sort_by_key(|p| std::cmp::Reverse(p.added_ms));
    for p in &photos {
        ensure(lib, p.image_ref.clone());
    }
    let thumbs = photos
        .into_iter()
        .map(|p| {
            let alt = EN.photo_alt.replace("{date}", &date_label(p.date));
            let open = p.clone();
            view! {
                <li>
                    <button class="photo-thumb" aria-label=alt.clone() on:click=move |_| viewing.set(Some(open.clone()))>
                        {image_view(p.image_ref.clone(), alt.clone(), "thumb")}
                    </button>
                    <small>{date_label(p.date)}</small>
                </li>
            }
        })
        .collect_view();
    let on_file = move |e: leptos::ev::Event| {
        let Some(input) = e
            .target()
            .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
        else {
            return;
        };
        let file = input.files().and_then(|f| f.get(0));
        input.set_value("");
        if let Some(file) = file {
            spawn_local(add(state, lib, photo_cache, id, file.into(), error));
        }
    };
    view! {
        <div class="care-section photos">
            <h3>{EN.photos_label}</h3>
            <ul class="photo-strip">{thumbs}</ul>
            <label class="add-photo">
                <input type="file" accept="image/*" capture="environment" on:change=on_file />
                <span>{EN.add_plant_photo}</span>
            </label>
            {move || error.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
            <p class="care-empty">{EN.photos_note}</p>
            {move || viewing.get().map(|p| {
                let alt = EN.photo_alt.replace("{date}", &date_label(p.date));
                let image_ref = p.image_ref.clone();
                view! {
                    <div class="photo-viewer" role="dialog" aria-label=alt.clone()
                        on:click=move |_| viewing.set(None)>
                        <figure on:click=|e| e.stop_propagation()>
                            {image_view(p.image_ref.clone(), alt.clone(), "large")}
                            <figcaption>
                                <span>{date_label(p.date)}</span>
                                <button on:click=move |_| {
                                    viewing.set(None);
                                    state.update(|s| s.remove_photo(id, &image_ref, now()));
                                }>{EN.delete}</button>
                                <button on:click=move |_| viewing.set(None)>{EN.close}</button>
                            </figcaption>
                        </figure>
                    </div>
                }
            })}
        </div>
    }
}

/// Every image reference a design uses.
fn refs_of(doc: &loma_core::model::Document, into: &mut HashSet<String>) {
    if let Some(bg) = &doc.background {
        into.insert(bg.image_ref.clone());
    }
    for care in doc.care.values() {
        into.extend(care.photos.iter().map(|p| p.image_ref.clone()));
    }
}

/// When a `photo-<ms>-…` reference was made, if it says.
fn made_at(key: &str) -> Option<i64> {
    key.strip_prefix("photo-")?.split('-').next()?.parse().ok()
}

/// Delete plant photos that no saved design uses any more. Runs once at start-up.
/// If any design can't be read, nothing is deleted.
pub async fn clean_up(store: Store, records: Vec<StoredDesign>, state: RwSignal<EditorState>) {
    let mut used = HashSet::new();
    for r in &records {
        match crate::library::parse(r) {
            Ok(doc) => refs_of(&doc, &mut used),
            Err(_) => return,
        }
    }
    state.with_untracked(|s| refs_of(s.doc(), &mut used));
    let Ok(keys) = store.list_image_keys().await else {
        return;
    };
    let now = now();
    for key in keys {
        let old_enough = made_at(&key).is_some_and(|t| now - t > KEEP_NEW_MS);
        if key.starts_with("photo-") && old_enough && !used.contains(&key) {
            let _ = store.delete_image(&key).await;
        }
    }
}
