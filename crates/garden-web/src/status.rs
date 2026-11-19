//! Bottom bar: cursor position, zoom, units and snapping.

use garden_core::units::{UnitSystem, format_length};
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use garden_editor::viewport::Viewport;
use leptos::prelude::*;

use crate::app::{Cursor, now};

#[component]
pub fn StatusBar() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let Cursor(cursor) = expect_context::<Cursor>();
    let units = Memo::new(move |_| state.with(|s| s.doc().units));
    let zoom = Memo::new(move |_| {
        state.with(|s| (s.viewport.scale / Viewport::DEFAULT_SCALE * 100.0).round())
    });
    let snap_grid = Memo::new(move |_| state.with(|s| s.snap_grid));
    let snap_objects = Memo::new(move |_| state.with(|s| s.snap_objects));

    let coords = move || {
        cursor.get().map(|p| {
            let u = units.get();
            format!("x {}  y {}", format_length(p.x, u), format_length(p.y, u))
        })
    };
    let set_units = move |u: UnitSystem| state.update(|s| s.set_units(u, now()));

    view! {
        <footer class="status">
            <span class="coords">{coords}</span>
            <span>{move || format!("{}%", zoom.get())}</span>
            <span>
                <button aria-pressed=move || (units.get() == UnitSystem::Metric).to_string()
                    on:click=move |_| set_units(UnitSystem::Metric)>{EN.units_metric}</button>
                <button aria-pressed=move || (units.get() == UnitSystem::Imperial).to_string()
                    on:click=move |_| set_units(UnitSystem::Imperial)>{EN.units_imperial}</button>
            </span>
            <label>
                <input type="checkbox" prop:checked=move || snap_grid.get()
                    on:change=move |e| state.update(|s| s.snap_grid = event_target_checked(&e)) />
                {EN.snap_grid}
            </label>
            <label>
                <input type="checkbox" prop:checked=move || snap_objects.get()
                    on:change=move |e| state.update(|s| s.snap_objects = event_target_checked(&e)) />
                {EN.snap_objects}
            </label>
        </footer>
    }
}
