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
pub fn now() -> i64 {
    js_sys::Date::now() as i64
}

/// World position of the pointer, for the status bar.
#[derive(Clone, Copy)]
pub struct Cursor(pub RwSignal<Option<Point>>);

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
    provide_context(SpaceHeld(space));
    provide_context(LastPlant(plant));
    crate::library::provide_library(state);
    crate::pwa::provide_pwa(state, expect_context::<crate::library::Library>());
    crate::trace::provide_trace();
    crate::view3d::provide_view3d();
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
