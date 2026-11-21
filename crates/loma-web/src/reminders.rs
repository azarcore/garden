//! Reminders without a server: the due summary the offline cache reads, the
//! count on the app icon, the "due today" banner, the calendar export and the
//! "Remind me" switch.

use leptos::prelude::*;
use loma_editor::reminders::{DueItem, due_count, due_items, summary_count, to_ics};
use loma_editor::state::{EditorState, Mode};
use loma_editor::strings::EN;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::app::now;
use crate::garden::today;
use crate::storage::StoredDesign;

const DATA_CACHE: &str = "loma-data";
const SUMMARY: &str = "loma-due.json";
const BANNER_KEY: &str = "loma-banner-closed";
const REMIND_KEY: &str = "loma-remind";
const SYNC_TAG: &str = "loma-due";

fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

fn local() -> Option<web_sys::Storage> {
    window().and_then(|w| w.local_storage().ok().flatten())
}

fn get(target: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(target, &key.into())
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

/// Call `target[name](...args)` if it exists; the result, if any.
fn call(target: &JsValue, name: &str, args: &[JsValue]) -> Option<JsValue> {
    let f = get(target, name)?.dyn_into::<js_sys::Function>().ok()?;
    let args: js_sys::Array = args.iter().collect();
    f.apply(target, &args).ok()
}

/// The summary file's address, next to the app (works in a sub-folder too).
fn summary_url() -> Option<String> {
    let base = window()?.document()?.base_uri().ok().flatten()?;
    web_sys::Url::new_with_base(SUMMARY, &base)
        .ok()
        .map(|u| u.href())
}

/// Write the due summary of every stored design, and update the icon count.
pub async fn write_summary(records: Vec<StoredDesign>) {
    let day = today();
    let items: Vec<DueItem> = records
        .iter()
        .filter_map(|r| crate::library::parse(r).ok())
        .flat_map(|doc| due_items(&doc, day))
        .collect();
    set_badge(summary_count(&items, day));
    let (Some(w), Some(url)) = (window(), summary_url()) else {
        return;
    };
    let Ok(value) = serde_wasm_bindgen::to_value(&Summary { items: &items }) else {
        return;
    };
    let Ok(json) = js_sys::JSON::stringify(&value) else {
        return;
    };
    let Ok(caches) = w.caches() else { return };
    let Ok(cache) = JsFuture::from(caches.open(DATA_CACHE)).await else {
        return;
    };
    let cache: web_sys::Cache = cache.unchecked_into();
    let init = web_sys::ResponseInit::new();
    if let Ok(headers) = web_sys::Headers::new() {
        let _ = headers.set("content-type", "application/json");
        init.set_headers(&headers);
    }
    let Ok(response) =
        web_sys::Response::new_with_opt_str_and_init(Some(&String::from(json)), &init)
    else {
        return;
    };
    let _ = JsFuture::from(cache.put_with_str(&url, &response)).await;
}

/// `{ "items": [...] }`, the shape sw.js reads. A struct, not a map: maps
/// become JavaScript `Map`s, which JSON.stringify writes as `{}`.
#[derive(serde::Serialize)]
struct Summary<'a> {
    items: &'a [DueItem],
}

/// The number on the app icon, where the system supports it.
fn set_badge(count: usize) {
    let Some(w) = window() else { return };
    let nav: JsValue = w.navigator().into();
    let promise = if count > 0 {
        call(&nav, "setAppBadge", &[JsValue::from(count as u32)])
    } else {
        call(&nav, "clearAppBadge", &[])
    };
    // a refused badge (no permission) is not an error worth showing
    if let Some(p) = promise.and_then(|p| p.dyn_into::<js_sys::Promise>().ok()) {
        leptos::task::spawn_local(async move {
            let _ = JsFuture::from(p).await;
        });
    }
}

/// "3 garden jobs due today · Show", once a day until closed.
#[component]
pub fn DueBanner() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ui = expect_context::<crate::care::CareUi>();
    let closed_on = RwSignal::new(local().and_then(|s| s.get_item(BANNER_KEY).ok().flatten()));
    let count = Memo::new(move |_| {
        let _ = state.with(|s| s.change_count());
        state.with(|s| due_count(s.doc(), today()))
    });
    let visible = move || count.get() > 0 && closed_on.get() != Some(today().to_string());
    let text = move || match count.get() {
        1 => EN.banner_due_one.to_string(),
        n => EN.banner_due.replace("{n}", &n.to_string()),
    };
    view! {
        <Show when=visible>
            <div class="card due-banner" role="status">
                <span>{text}</span>
                <button class="due-show" on:click=move |_| {
                    crate::garden::set_mode(state, Mode::Garden);
                    ui.week_open.set(true);
                }>{EN.show}</button>
                <button class="icon-btn" aria-label=EN.close on:click=move |_| {
                    let day = today().to_string();
                    if let Some(s) = local() {
                        let _ = s.set_item(BANNER_KEY, &day);
                    }
                    closed_on.set(Some(day));
                }>{crate::icons::icon(crate::icons::Icon::Close)}</button>
            </div>
        </Show>
    }
}

/// Offer the calendar file: the share sheet where it takes files (installed
/// iPhone apps), otherwise a download.
pub fn export_calendar(state: RwSignal<EditorState>) {
    let (name, ics) =
        state.with_untracked(|s| (s.doc().name.clone(), to_ics(s.doc(), today(), now())));
    let file_name = format!("{name} {}.ics", EN.calendar_file);
    let download = {
        let (file_name, ics) = (file_name.clone(), ics.clone());
        move || {
            crate::files::download(
                &file_name,
                "text/calendar;charset=utf-8",
                crate::files::Download::Text(ics),
            )
        }
    };
    let Some(w) = window() else { return };
    let nav: JsValue = w.navigator().into();
    let parts: js_sys::Array = [JsValue::from_str(&ics)].into_iter().collect();
    let opts = web_sys::FilePropertyBag::new();
    opts.set_type("text/calendar");
    let Ok(file) = web_sys::File::new_with_str_sequence_and_options(&parts, &file_name, &opts)
    else {
        return download();
    };
    let data = js_sys::Object::new();
    let files: js_sys::Array = [JsValue::from(file)].into_iter().collect();
    let _ = js_sys::Reflect::set(&data, &"files".into(), &files);
    let can = call(&nav, "canShare", &[data.clone().into()]).is_some_and(|v| v.is_truthy());
    if !can {
        return download();
    }
    let Some(promise) =
        call(&nav, "share", &[data.into()]).and_then(|p| p.dyn_into::<js_sys::Promise>().ok())
    else {
        return download();
    };
    leptos::task::spawn_local(async move {
        if let Err(e) = JsFuture::from(promise).await {
            // cancelling the share sheet is the person's choice; other failures fall back
            let aborted =
                get(&e, "name").and_then(|n| n.as_string()).as_deref() == Some("AbortError");
            if !aborted {
                download();
            }
        }
    });
}

/// What the device can do for reminders.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemindKind {
    /// Background checks exist (Android Chrome, installed): daily reminder and badge.
    Daily,
    /// Only notifications permission (for the icon count).
    BadgeOnly,
}

async fn registration() -> Option<web_sys::ServiceWorkerRegistration> {
    let reg = JsFuture::from(window()?.navigator().service_worker().ready().ok()?)
        .await
        .ok()?;
    reg.dyn_into().ok()
}

/// None when the browser has no notifications at all. Daily only where the
/// browser actually allows background checks (an installed app), not merely
/// where the API exists.
pub async fn remind_kind() -> Option<RemindKind> {
    let w = window()?;
    get(&w.clone().into(), "Notification")?;
    let has_sync = registration()
        .await
        .and_then(|r| get(&r.into(), "periodicSync"))
        .is_some();
    Some(if has_sync && periodic_sync_allowed(&w).await {
        RemindKind::Daily
    } else {
        RemindKind::BadgeOnly
    })
}

async fn periodic_sync_allowed(w: &web_sys::Window) -> bool {
    let Some(permissions) = get(&w.navigator().into(), "permissions") else {
        return false;
    };
    let query = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&query, &"name".into(), &"periodic-background-sync".into());
    let Some(p) = call(&permissions, "query", &[query.into()])
        .and_then(|p| p.dyn_into::<js_sys::Promise>().ok())
    else {
        return false;
    };
    match JsFuture::from(p).await {
        Ok(status) => {
            get(&status, "state").and_then(|s| s.as_string()).as_deref() == Some("granted")
        }
        Err(_) => false,
    }
}

pub fn remind_on() -> bool {
    local()
        .and_then(|s| s.get_item(REMIND_KEY).ok().flatten())
        .as_deref()
        == Some("on")
}

/// Turn reminders on: ask permission, then register the daily check.
/// Returns false when permission was refused.
pub async fn turn_on() -> bool {
    let Ok(p) = web_sys::Notification::request_permission() else {
        return false;
    };
    let granted = JsFuture::from(p)
        .await
        .ok()
        .and_then(|v| v.as_string())
        .as_deref()
        == Some("granted");
    if !granted {
        return false;
    }
    if let Some(reg) = registration().await
        && let Some(sync) = get(&reg.into(), "periodicSync")
    {
        let opts = js_sys::Object::new();
        let _ = js_sys::Reflect::set(
            &opts,
            &"minInterval".into(),
            &JsValue::from_f64(86_400_000.0),
        );
        if let Some(p) = call(&sync, "register", &[SYNC_TAG.into(), opts.into()])
            .and_then(|p| p.dyn_into::<js_sys::Promise>().ok())
        {
            let _ = JsFuture::from(p).await;
        }
    }
    if let Some(s) = local() {
        let _ = s.set_item(REMIND_KEY, "on");
    }
    true
}

pub async fn turn_off() {
    if let Some(reg) = registration().await
        && let Some(sync) = get(&reg.into(), "periodicSync")
        && let Some(p) = call(&sync, "unregister", &[SYNC_TAG.into()])
            .and_then(|p| p.dyn_into::<js_sys::Promise>().ok())
    {
        let _ = JsFuture::from(p).await;
    }
    if let Some(s) = local() {
        let _ = s.remove_item(REMIND_KEY);
    }
}

/// The calendar button and the reminder switch, under the This week list.
#[component]
pub fn ReminderControls() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let kind = RwSignal::new(None::<RemindKind>);
    let on = RwSignal::new(remind_on());
    let refused = RwSignal::new(false);
    leptos::task::spawn_local(async move {
        kind.set(remind_kind().await);
    });
    let toggle = move |e: leptos::ev::Event| {
        let want = event_target_checked(&e);
        leptos::task::spawn_local(async move {
            if want {
                let ok = turn_on().await;
                refused.set(!ok);
                on.set(ok);
            } else {
                turn_off().await;
                refused.set(false);
                on.set(false);
            }
        });
    };
    view! {
        <div class="reminders">
            <button class="trace-secondary add-calendar" on:click=move |_| export_calendar(state)>
                {crate::icons::icon(crate::icons::Icon::Calendar)}{EN.add_to_calendar}
            </button>
            <p class="care-empty">{EN.calendar_hint}</p>
            {move || kind.get().map(|k| view! {
                <label class="remind-switch">
                    <input type="checkbox" role="switch" prop:checked=move || on.get() on:change=toggle />
                    <span>{if k == RemindKind::Daily { EN.remind_daily } else { EN.remind_badge }}</span>
                </label>
                {(k == RemindKind::Daily).then(|| view! { <p class="care-empty">{EN.remind_best_effort}</p> })}
            })}
            {move || refused.get().then(|| view! { <p class="file-message" role="alert">{EN.remind_refused}</p> })}
        </div>
    }
}
