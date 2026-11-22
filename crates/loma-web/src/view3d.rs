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
    /// The moment the sun is shown for, in local clock time: year, day of the
    /// year (1-based) and minute of the day. Not saved; starts at now.
    sun_year: RwSignal<i32>,
    sun_day: RwSignal<u32>,
    sun_minute: RwSignal<u32>,
    /// Whose clock the times are in: the garden's zone, or `None` for the device's.
    clock: RwSignal<Option<String>>,
}

pub fn provide_view3d() {
    let view = View3d {
        open: RwSignal::new(false),
        status: RwSignal::new(Status::Idle),
        module: StoredValue::new_local(None),
        sun_year: RwSignal::new(2026),
        sun_day: RwSignal::new(1),
        sun_minute: RwSignal::new(12 * 60),
        clock: RwSignal::new(None),
    };
    sun_to_now(view);
    provide_context(view);
}

// ---- clock time: the garden's zone, or the device's when `zone` is None ----

const DAY_MS: f64 = 86_400_000.0;

/// A zone's offset from UTC (ms) at `utc_ms`; 0 if the browser can't say.
fn offset(zone: &str, utc_ms: f64) -> f64 {
    crate::app::zone_offset_ms(zone, utc_ms).unwrap_or(0.0)
}

/// The moment of a clock time; days past the year's end roll on.
fn local_ms(year: i32, day: u32, minute: u32, zone: Option<&str>) -> f64 {
    match zone {
        None => js_sys::Date::new_with_year_month_day_hr_min(
            year as u32,
            0,
            day as i32,
            (minute / 60) as i32,
            (minute % 60) as i32,
        )
        .get_time(),
        Some(z) => {
            let wall = js_sys::Date::utc(year as f64, 0.0)
                + (day as f64 - 1.0) * DAY_MS
                + minute as f64 * 60_000.0;
            loma_editor::timezones::local_to_utc(wall, |t| offset(z, t))
        }
    }
}

/// The day of the year (1-based) of a date (`month` 1-based).
fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    let jan1 = js_sys::Date::utc(year as f64, 0.0);
    let date = js_sys::Date::utc(year as f64, month as f64 - 1.0) + (day as f64 - 1.0) * DAY_MS;
    ((date - jan1) / DAY_MS).round() as u32 + 1
}

fn days_in_year(year: i32) -> u32 {
    if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
        366
    } else {
        365
    }
}

/// The clock reading of a moment, written as if it were UTC (so its UTC
/// fields are the clock's fields).
fn wall(ms: f64, zone: Option<&str>) -> js_sys::Date {
    let shifted = match zone {
        Some(z) => ms + offset(z, ms),
        None => ms - js_sys::Date::new(&JsValue::from_f64(ms)).get_timezone_offset() * 60_000.0,
    };
    js_sys::Date::new(&JsValue::from_f64(shifted))
}

/// The clock time of a moment, in minutes since midnight.
fn clock_minute(ms: f64, zone: Option<&str>) -> u32 {
    let d = wall(ms, zone);
    d.get_utc_hours() * 60 + d.get_utc_minutes()
}

fn clock_text(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// "21 Jun" in the person's own language.
fn date_text(year: i32, day: u32) -> String {
    let options = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&options, &"day".into(), &"numeric".into());
    let _ = js_sys::Reflect::set(&options, &"month".into(), &"short".into());
    let _ = js_sys::Reflect::set(&options, &"timeZone".into(), &"UTC".into());
    let noon = js_sys::Date::utc(year as f64, 0.0) + (day as f64 - 1.0) * DAY_MS + DAY_MS / 2.0;
    js_sys::Date::new(&JsValue::from_f64(noon))
        .to_locale_date_string("default", &options)
        .into()
}

/// Show the sun as it is right now, in whichever clock is chosen.
fn sun_to_now(view: View3d) {
    let zone = view.clock.get_untracked();
    let now = wall(js_sys::Date::now(), zone.as_deref());
    let year = now.get_utc_full_year() as i32;
    view.sun_year.set(year);
    view.sun_day.set(day_of_year(
        year,
        now.get_utc_month() + 1,
        now.get_utc_date(),
    ));
    // to the slider's 5-minute steps, so its thumb agrees with the label
    let minute = (now.get_utc_hours() * 60 + now.get_utc_minutes() + 2) / 5 * 5;
    view.sun_minute.set(minute.min(1435));
}

/// Light the open 3D view from the sun at the chosen moment, or as before
/// when the design has no location.
fn light_3d(view: View3d, place: Option<(loma_core::model::GeoPoint, f64)>) {
    let Some(module) = view.module.get_value() else {
        return;
    };
    let _ = match place {
        Some((at, north)) => {
            let ms = local_ms(
                view.sun_year.get_untracked(),
                view.sun_day.get_untracked(),
                view.sun_minute.get_untracked(),
                view.clock.get_untracked().as_deref(),
            );
            let sun = loma_core::sun::sun_position(at, ms);
            let bearing = loma_editor::compass::sun_bearing_on_plan(north, sun.azimuth);
            call(
                &module,
                "set_sun",
                &[
                    &JsValue::from_f64(bearing),
                    &JsValue::from_f64(sun.altitude),
                ],
            )
        }
        None => call(&module, "default_sun", &[]),
    };
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
    sun_to_now(view);
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
    let state = expect_context::<RwSignal<EditorState>>();
    let compass_box = expect_context::<crate::app::CompassBox>();
    let place = Memo::new(move |_| state.with(|s| s.doc().location.map(|at| (at, s.doc().north))));
    let design = Memo::new(move |_| state.with(|s| s.mode() == loma_editor::state::Mode::Design));
    // the garden's zone when it has one the browser knows and the person
    // hasn't chosen their own clock
    let garden_zone = Memo::new(move |_| {
        state.with(|s| {
            s.doc()
                .time_zone
                .clone()
                .filter(|z| crate::app::zone_offset_ms(z, 0.0).is_some())
        })
    });
    let device_time = Memo::new(move |_| state.with(|s| s.doc().device_time));
    Effect::new(move |_| {
        let clock = garden_zone.get().filter(|_| !device_time.get());
        if clock != view.clock.get_untracked() {
            view.clock.set(clock);
            sun_to_now(view); // a new clock: start again from now
        }
    });
    // relight whenever the view is showing and the place or moment changes
    Effect::new(move |_| {
        let showing = view.open.get() && view.status.get() == Status::Ready;
        let place = place.get();
        let _moment = (
            view.sun_year.get(),
            view.sun_day.get(),
            view.sun_minute.get(),
            view.clock.get(),
        );
        if showing {
            light_3d(view, place);
        }
    });
    // sunrise and sunset on the chosen day, as local clock minutes
    let sun_times = Memo::new(move |_| {
        let (at, _) = place.get()?;
        let (year, day) = (view.sun_year.get(), view.sun_day.get());
        let zone = view.clock.get();
        let zone = zone.as_deref();
        let (rise, set) = loma_core::sun::sun_times(
            at,
            local_ms(year, day, 0, zone),
            local_ms(year, day + 1, 0, zone),
        );
        Some((
            rise.map(|t| clock_minute(t, zone)),
            set.map(|t| clock_minute(t, zone)),
        ))
    });
    let preset = move |month: u32, day: u32| {
        let year = view.sun_year.get_untracked();
        view.sun_day.set(day_of_year(year, month, day));
    };
    let south = move || place.get_untracked().is_some_and(|(at, _)| at.lat < 0.0);
    let has_place = Memo::new(move |_| place.get().is_some());
    let rise = move || sun_times.get().and_then(|t| t.0);
    let set = move || sun_times.get().and_then(|t| t.1);
    let tick = move |minute: Option<u32>, class: &'static str| {
        minute.map(|m| {
            view! {
                <span class=class style=format!("left:{:.2}%", m as f64 / 1439.0 * 100.0)
                    aria-hidden="true"></span>
            }
        })
    };
    let days = move || days_in_year(view.sun_year.get());
    // rebuilt only when a location appears or goes, so dragging a slider
    // never replaces the slider under the pointer
    let sun_panel = move || {
        if !has_place.get() {
            return design.get().then(|| {
                view! {
                    <button class="sun-link" on:click=move |_| {
                        view.open.set(false);
                        compass_box.at_place.set(true);
                        compass_box.open.set(true);
                    }>{EN.sun_link}</button>
                }
                .into_any()
            });
        }
        Some(
            view! {
                <div class="sun-panel">
                    {move || garden_zone.get().map(|z| {
                        let city = loma_editor::timezones::zone_city(&z);
                        let choose = move |device: bool| {
                            state.update(|s| { s.set_device_time(device, crate::app::now()); });
                        };
                        view! {
                            <div class="sun-clock" role="group" aria-label=EN.time_zone>
                                <button aria-pressed=move || (!device_time.get()).to_string()
                                    on:click=move |_| choose(false)>
                                    {EN.garden_time.replace("{}", &city)}</button>
                                <button aria-pressed=move || device_time.get().to_string()
                                    on:click=move |_| choose(true)>{EN.your_time}</button>
                            </div>
                        }
                    })}
                    <div class="sun-presets">
                        <button on:click=move |_| sun_to_now(view)>{EN.sun_now}</button>
                        <button on:click=move |_| if south() { preset(12, 21) } else { preset(6, 21) }>
                            {EN.sun_midsummer}</button>
                        <button on:click=move |_| preset(3, 20)>{EN.sun_equinox}</button>
                        <button on:click=move |_| if south() { preset(6, 21) } else { preset(12, 21) }>
                            {EN.sun_midwinter}</button>
                    </div>
                    <label class="sun-row">
                        <span class="sun-label">{EN.sun_time}</span>
                        <span class="sun-track">
                            <input type="range" min="0" max="1439" step="5"
                                prop:value=move || view.sun_minute.get().to_string()
                                on:input=move |e| {
                                    if let Ok(m) = event_target_value(&e).parse::<u32>() {
                                        view.sun_minute.set(m.min(1439));
                                    }
                                } />
                            {move || tick(rise(), "sun-tick rise")}
                            {move || tick(set(), "sun-tick set")}
                        </span>
                        <output class="sun-value">{move || clock_text(view.sun_minute.get())}</output>
                    </label>
                    <label class="sun-row">
                        <span class="sun-label">{EN.sun_date}</span>
                        <span class="sun-track">
                            <input type="range" min="1" max=move || days().to_string()
                                prop:value=move || view.sun_day.get().to_string()
                                on:input=move |e| {
                                    if let Ok(d) = event_target_value(&e).parse::<u32>() {
                                        view.sun_day.set(d.clamp(1, days()));
                                    }
                                } />
                        </span>
                        <output class="sun-value">
                            {move || date_text(view.sun_year.get(), view.sun_day.get())}
                        </output>
                    </label>
                    <p class="sun-times">
                        <span>{move || match rise() {
                            Some(m) => format!("{} {}", EN.sun_rise, clock_text(m)),
                            None => EN.sun_no_rise.to_string(),
                        }}</span>
                        <span>{move || match set() {
                            Some(m) => format!("{} {}", EN.sun_set, clock_text(m)),
                            None => EN.sun_no_set.to_string(),
                        }}</span>
                    </p>
                </div>
            }
            .into_any(),
        )
    };
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
            <p class="view3d-help" class:above-sun=move || has_place.get()>{help}</p>
            {sun_panel}
        </section>
    }
}
