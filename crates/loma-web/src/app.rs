use leptos::ev;
use leptos::prelude::*;
use loma_core::geometry::Point;
use loma_core::model::{Document, PlantDef};
use loma_editor::catalog::builtin_plants;
use loma_editor::state::{EditorState, Mode};
use loma_editor::strings::EN;
use loma_editor::tools::{Input, Key, Modifiers};
use wasm_bindgen::JsCast;

use crate::canvas::Canvas;
use crate::inspector::Inspector;
use crate::toolbar::{ControlsCard, HintBar, TitleCard, ToolDock, tool_entries};

/// Milliseconds since the Unix epoch.
/// Ask the browser where this device is (once; it may ask the person) and
/// call `done` with latitude and longitude, or `None` if it can't or may not.
/// The answer stays on the device.
pub fn current_position(done: impl FnOnce(Option<(f64, f64)>) + 'static) {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::prelude::Closure;
    type Done = Rc<RefCell<Option<Box<dyn FnOnce(Option<(f64, f64)>)>>>>;
    let done: Done = Rc::new(RefCell::new(Some(Box::new(done))));
    let finish = |done: &Done, found| {
        if let Some(f) = done.borrow_mut().take() {
            f(found);
        }
    };
    let get = |o: &JsValue, key: &str| js_sys::Reflect::get(o, &JsValue::from_str(key)).ok();
    let geolocation = web_sys::window()
        .map(|w| JsValue::from(w.navigator()))
        .and_then(|n| get(&n, "geolocation"))
        .filter(|g| !g.is_undefined() && !g.is_null());
    let Some(geolocation) = geolocation else {
        finish(&done, None);
        return;
    };
    let on_found = {
        let done = done.clone();
        Closure::once_into_js(move |position: JsValue| {
            let coords = get(&position, "coords");
            let read = |k| {
                coords
                    .as_ref()
                    .and_then(|c| get(c, k))
                    .and_then(|v| v.as_f64())
            };
            finish(&done, read("latitude").zip(read("longitude")));
        })
    };
    let on_error = {
        let done = done.clone();
        Closure::once_into_js(move |_: JsValue| finish(&done, None))
    };
    // some browsers never answer when the permission prompt is ignored
    let give_up = {
        let done = done.clone();
        Closure::once_into_js(move || finish(&done, None))
    };
    if let Some(w) = web_sys::window() {
        let _ = w
            .set_timeout_with_callback_and_timeout_and_arguments_0(give_up.unchecked_ref(), 20_000);
    }
    let options = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&options, &"enableHighAccuracy".into(), &false.into());
    let _ = js_sys::Reflect::set(&options, &"timeout".into(), &15_000.into());
    let _ = js_sys::Reflect::set(&options, &"maximumAge".into(), &600_000.into());
    let called = get(&geolocation, "getCurrentPosition")
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .map(|f| f.call3(&geolocation, &on_found, &on_error, &options));
    if !matches!(called, Some(Ok(_))) {
        finish(&done, None);
    }
}

/// The device's own time zone (an IANA name), if the browser says.
pub fn device_zone() -> Option<String> {
    let format = js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &js_sys::Object::new());
    js_sys::Reflect::get(&format.resolved_options(), &"timeZone".into())
        .ok()?
        .as_string()
}

thread_local! {
    /// One formatter per zone (making them is slow); `None` for zones the
    /// browser doesn't know.
    static ZONE_FORMATS: std::cell::RefCell<std::collections::HashMap<String, Option<js_sys::Intl::DateTimeFormat>>> =
        Default::default();
}

/// How far `zone`'s clocks are ahead of UTC (ms) at moment `utc_ms`, using
/// the browser's own rules (summer time included). `None` for a zone the
/// browser doesn't know.
pub fn zone_offset_ms(zone: &str, utc_ms: f64) -> Option<f64> {
    use wasm_bindgen::JsValue;
    let format = ZONE_FORMATS.with(|cache| {
        cache
            .borrow_mut()
            .entry(zone.to_string())
            .or_insert_with(|| {
                let options = js_sys::Object::new();
                let set = |k: &str, v: &str| {
                    let _ = js_sys::Reflect::set(&options, &k.into(), &v.into());
                };
                set("timeZone", zone);
                set("hourCycle", "h23");
                for part in ["year", "month", "day", "hour", "minute", "second"] {
                    set(part, "numeric");
                }
                // the constructor throws for an unknown zone: catch it
                let intl = js_sys::Reflect::get(&js_sys::global(), &"Intl".into()).ok()?;
                let ctor: js_sys::Function = js_sys::Reflect::get(&intl, &"DateTimeFormat".into())
                    .ok()?
                    .dyn_into()
                    .ok()?;
                let args = js_sys::Array::of2(&"en-US".into(), &options);
                js_sys::Reflect::construct(&ctor, &args)
                    .ok()
                    .map(|f| f.unchecked_into())
            })
            .clone()
    })?;
    // round to the second: the parts below have no milliseconds
    let utc = (utc_ms / 1000.0).floor() * 1000.0;
    let parts = format.format_to_parts(&js_sys::Date::new(&JsValue::from_f64(utc)));
    let mut v = [0.0f64; 6];
    for p in parts.iter() {
        let kind = js_sys::Reflect::get(&p, &"type".into()).ok()?.as_string()?;
        let i = match kind.as_str() {
            "year" => 0,
            "month" => 1,
            "day" => 2,
            "hour" => 3,
            "minute" => 4,
            "second" => 5,
            _ => continue, // the "/", ", " and ":" between them
        };
        v[i] = js_sys::Reflect::get(&p, &"value".into())
            .ok()?
            .as_string()?
            .parse()
            .ok()?;
    }
    let wall = js_sys::Date::utc(v[0], v[1] - 1.0) + 0.0; // year and month only
    // some engines once wrote midnight as hour 24
    let hour = v[3] % 24.0;
    let wall =
        wall + (v[2] - 1.0) * 86_400_000.0 + hour * 3_600_000.0 + v[4] * 60_000.0 + v[5] * 1000.0;
    Some(wall - utc)
}

pub fn now() -> i64 {
    js_sys::Date::now() as i64
}

/// World position of the pointer, for the status bar.
#[derive(Clone, Copy)]
pub struct Cursor(pub RwSignal<Option<Point>>);

/// Whether the compass's box (north and location) is open; the 3D view's
/// "set where your garden is" link opens it.
#[derive(Clone, Copy)]
pub struct CompassBox {
    pub open: RwSignal<bool>,
    /// Open it with the Location field focused (from the 3D view's link).
    pub at_place: RwSignal<bool>,
}

/// Whether Space is held (pan mode).
#[derive(Clone, Copy)]
pub struct SpaceHeld(pub RwSignal<bool>);

/// The plant the plant tool places.
#[derive(Clone, Copy)]
pub struct LastPlant(pub RwSignal<PlantDef>);

/// True when the key went to a text field, so shortcuts must not fire.
fn typing_in_field(e: &ev::KeyboardEvent) -> bool {
    let Some(el) = e
        .target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok())
    else {
        return false;
    };
    matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
        || el.is_content_editable()
        // the plant form's own buttons (symbols, Save, Cancel) aren't editor keys
        || el.closest(".plant-form").ok().flatten().is_some()
        // nor the compass's bearing box (Reset must take Enter)
        || el.closest(".compass-edit").ok().flatten().is_some()
        // nor are the garden list's buttons
        || el.closest(".list-card").ok().flatten().is_some()
        // nor the job and product forms' buttons (the rest of the plant card
        // keeps Ctrl+Z, so a health change can be undone straight away)
        || el.closest(".log-form").ok().flatten().is_some()
        || el.closest(".product-form").ok().flatten().is_some()
        || el.closest(".task-form").ok().flatten().is_some()
}

/// The panels and modes the keyboard handler needs to know about.
#[derive(Clone, Copy)]
struct Panels {
    designs_open: RwSignal<bool>,
    view3d: crate::view3d::View3d,
    chooser: RwSignal<bool>,
    catalog_open: RwSignal<bool>,
}

fn on_key_down(
    e: ev::KeyboardEvent,
    state: RwSignal<EditorState>,
    space: RwSignal<bool>,
    plant: RwSignal<PlantDef>,
    panels: Panels,
) {
    let Panels {
        designs_open,
        view3d,
        chooser,
        catalog_open,
    } = panels;
    // The chooser is a dialog: Escape closes it, other shortcuts wait.
    if chooser.get_untracked() {
        if e.key() == "Escape" {
            chooser.set(false);
        }
        return;
    }
    // The 3D view covers the editor: only Escape (close it) does anything, so
    // shortcuts can't change the design hidden behind it.
    if view3d.open.get_untracked() {
        if e.key() == "Escape" {
            e.prevent_default();
            view3d.open.set(false);
        }
        return;
    }
    if typing_in_field(&e) {
        return;
    }
    // With My designs open, Escape just closes the panel (it must not cancel a drawing).
    if e.key() == "Escape" && designs_open.get_untracked() {
        return;
    }
    if state.with_untracked(|s| s.mode()) == Mode::Garden {
        garden_key(&e, state, view3d);
        return;
    }
    let m = Modifiers {
        shift: e.shift_key(),
        alt: e.alt_key(),
        ctrl: e.ctrl_key() || e.meta_key(),
    };
    let key = e.key();
    let input_key = match key.as_str() {
        "Escape" => Some(Key::Escape),
        "Enter" => Some(Key::Enter),
        "Delete" => Some(Key::Delete),
        "Backspace" => Some(Key::Backspace),
        _ => None,
    };
    if let Some(k) = input_key {
        e.prevent_default();
        state.update(|s| s.handle(Input::Key(k, m), now()));
        return;
    }
    let lower = key.to_ascii_lowercase();
    if m.ctrl {
        match lower.as_str() {
            "z" if m.shift => state.update(|s| {
                s.redo(now());
            }),
            "z" => state.update(|s| {
                s.undo(now());
            }),
            "y" => state.update(|s| {
                s.redo(now());
            }),
            "d" => state.update(|s| s.duplicate_selection(now())),
            _ => return,
        }
        e.prevent_default();
        return;
    }
    if key == " " {
        e.prevent_default();
        space.set(true);
        return;
    }
    if m.alt {
        return;
    }
    if key == "/" {
        e.prevent_default();
        crate::catalog::focus_search(catalog_open);
        return;
    }
    if lower == "3" {
        crate::view3d::open_3d(view3d, state);
        return;
    }
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let c = c.to_ascii_uppercase();
        if let Some(entry) = tool_entries(plant.get_untracked())
            .into_iter()
            .find(|t| t.key == c)
        {
            state.update(|s| s.switch_tool(entry.tool, now()));
        }
    }
}

/// Garden mode keys: undo/redo of care changes, Escape, Space to pan and 3 for 3D.
/// Nothing that edits the layout.
fn garden_key(e: &ev::KeyboardEvent, state: RwSignal<EditorState>, view3d: crate::view3d::View3d) {
    let key = e.key();
    let lower = key.to_ascii_lowercase();
    let ctrl = e.ctrl_key() || e.meta_key();
    match (ctrl, lower.as_str()) {
        (true, "z") if e.shift_key() => state.update(|s| {
            s.redo(now());
        }),
        (true, "z") => state.update(|s| {
            s.undo(now());
        }),
        (true, "y") => state.update(|s| {
            s.redo(now());
        }),
        (false, "escape") => state.update(|s| s.selection.clear()),
        (false, "3") if !e.alt_key() => {
            crate::view3d::open_3d(view3d, state);
            return;
        }
        _ => return,
    }
    e.prevent_default();
}

/// A reminder notification opens the app at `?week=1`: show This week in
/// Garden mode, then tidy the address.
fn open_week_from_link(state: RwSignal<EditorState>) {
    let Some(w) = web_sys::window() else { return };
    let search = w.location().search().unwrap_or_default();
    if !search.split(['?', '&']).any(|p| p == "week=1") {
        return;
    }
    crate::garden::set_mode(state, Mode::Garden);
    expect_context::<crate::care::CareUi>().week_open.set(true);
    if let (Ok(history), Ok(path)) = (w.history(), w.location().pathname()) {
        let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(&path));
    }
}

#[component]
pub fn App() -> impl IntoView {
    let state = RwSignal::new(EditorState::new(
        Document::new(EN.new_design_name, now()),
        1000.,
        700.,
    ));
    state.update(|s| s.set_mode(crate::garden::saved_mode()));
    provide_context(crate::garden::SelectSeveral(RwSignal::new(false)));
    crate::care::provide_care_ui();
    crate::photos::provide_photos();
    let space = RwSignal::new(false);
    let default_plant = builtin_plants()
        .into_iter()
        .find(|p| p.id == "lavender")
        .unwrap_or_else(|| builtin_plants()[0].clone());
    let plant = RwSignal::new(default_plant);
    provide_context(state);
    provide_context(Cursor(RwSignal::new(None)));
    provide_context(CompassBox {
        open: RwSignal::new(false),
        at_place: RwSignal::new(false),
    });
    provide_context(SpaceHeld(space));
    provide_context(LastPlant(plant));
    crate::library::provide_library(state);
    crate::pwa::provide_pwa(state, expect_context::<crate::library::Library>());
    crate::trace::provide_trace();
    crate::view3d::provide_view3d();
    crate::sunmap::provide_sun_map();
    crate::sunmap::run_sun_map();
    let catalog_open = RwSignal::new(false);
    provide_context(crate::catalog::CatalogOpen(catalog_open));
    let chooser = RwSignal::new(false);
    provide_context(crate::templates::ChooserOpen(chooser));
    let view3d = expect_context::<crate::view3d::View3d>();
    provide_context(crate::print::PrintOpen(RwSignal::new(false)));
    provide_context(crate::list::ListOpen(RwSignal::new(false)));
    provide_context(crate::layers::LayersOpen(RwSignal::new(false)));
    let crate::library::DesignsOpen(designs_open) = expect_context::<crate::library::DesignsOpen>();

    let design = move || state.with(|s| s.mode() == Mode::Design);
    open_week_from_link(state);
    let down = window_event_listener(ev::keydown, move |e| {
        let panels = Panels {
            designs_open,
            view3d,
            chooser,
            catalog_open,
        };
        on_key_down(e, state, space, plant, panels)
    });
    let up = window_event_listener(ev::keyup, move |e| {
        if e.key() == " " {
            space.set(false);
        }
    });
    // Space released while the window was in the background never sends keyup.
    let blur = window_event_listener(ev::blur, move |_| space.set(false));
    on_cleanup(move || {
        down.remove();
        up.remove();
        blur.remove();
    });

    view! {
        <div class="app">
            // While 3D covers it, the editor can't be reached by Tab or typing.
            <div class="editor" inert=move || view3d.open.get() || chooser.get()>
            <main class="canvas-cell">
                <Canvas />
            </main>
            <TitleCard />
            <crate::library::MyDesigns />
            <Show when=design><crate::trace::TraceCard /></Show>
            <crate::print::PrintCard />
            <crate::list::ListCard />
            <crate::sunmap::SunMapCard />
            <crate::care::PlantCard />
            <crate::care::ProductsCard />
            <crate::calendar::WeekCard />
            <crate::reminders::DueBanner />
            <Show when=design><crate::layers::LayersCard /></Show>
            <ControlsCard />
            <Show when=design>
                <crate::catalog::CatalogPanel />
                <Inspector />
                <HintBar />
                <ToolDock />
            </Show>
            <crate::templates::EmptyPlanPrompt />
            </div>
            <crate::templates::TemplateChooser />
            <crate::view3d::View3dOverlay />
        </div>
    }
}
