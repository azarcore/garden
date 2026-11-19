//! Tool buttons and document actions.

use garden_core::model::{AreaType, LinearKind, PlantDef, StructureKind};
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use garden_editor::tools::{DrawTarget, LineTarget, ToolKind};
use leptos::prelude::*;

use crate::app::{LastPlant, now};

/// A toolbar entry: label, keyboard shortcut and the tool it selects.
pub struct ToolEntry {
    pub label: &'static str,
    pub key: char,
    pub tool: ToolKind,
}

/// Every drawing tool, in toolbar order. The plant entry uses `plant`.
pub fn tool_entries(plant: PlantDef) -> Vec<ToolEntry> {
    let e = |label, key, tool| ToolEntry { label, key, tool };
    vec![
        e(EN.tool_select, 'V', ToolKind::Select),
        e(EN.tool_plot, 'K', ToolKind::Polygon(DrawTarget::Plot)),
        e(
            EN.tool_bed,
            'B',
            ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)),
        ),
        e(
            EN.tool_bed_rect,
            'R',
            ToolKind::Rect(DrawTarget::Area(AreaType::Bed)),
        ),
        e(
            EN.tool_lawn,
            'E',
            ToolKind::Ellipse(DrawTarget::Area(AreaType::Lawn)),
        ),
        e(
            EN.tool_paving,
            'A',
            ToolKind::Rect(DrawTarget::Area(AreaType::Paving)),
        ),
        e(
            EN.tool_gravel,
            'X',
            ToolKind::Polygon(DrawTarget::Area(AreaType::Gravel)),
        ),
        e(
            EN.tool_water,
            'O',
            ToolKind::Ellipse(DrawTarget::Area(AreaType::Water)),
        ),
        e(
            EN.tool_path,
            'P',
            ToolKind::Polyline(LineTarget::Path { width: 900.0 }),
        ),
        e(
            EN.tool_fence,
            'F',
            ToolKind::Polyline(LineTarget::Linear {
                kind: LinearKind::Fence,
                thickness: 50.0,
                height: 1800.0,
            }),
        ),
        e(
            EN.tool_wall,
            'W',
            ToolKind::Polyline(LineTarget::Linear {
                kind: LinearKind::Wall,
                thickness: 200.0,
                height: 1800.0,
            }),
        ),
        e(
            EN.tool_hedge,
            'H',
            ToolKind::Polyline(LineTarget::Linear {
                kind: LinearKind::Hedge,
                thickness: 600.0,
                height: 1500.0,
            }),
        ),
        e(
            EN.tool_structure,
            'S',
            ToolKind::Rect(DrawTarget::Structure(StructureKind::Shed)),
        ),
        e(EN.tool_plant, 'G', ToolKind::Plant(plant)),
    ]
}

/// Same tool, ignoring which plant is picked.
pub fn same_tool(a: &ToolKind, b: &ToolKind) -> bool {
    match (a, b) {
        (ToolKind::Plant(_), ToolKind::Plant(_)) => true,
        _ => a == b,
    }
}

#[component]
pub fn Toolbar() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let LastPlant(last_plant) = expect_context::<LastPlant>();
    let current = Memo::new(move |_| state.with(|s| s.tool().clone()));
    let can_undo = Memo::new(move |_| state.with(|s| s.can_undo()));
    let can_redo = Memo::new(move |_| state.with(|s| s.can_redo()));
    let has_selection = Memo::new(move |_| state.with(|s| !s.selection.is_empty()));

    let buttons = move || {
        tool_entries(last_plant.get())
            .into_iter()
            .map(|entry| {
                let tool = entry.tool.clone();
                let pressed = {
                    let tool = tool.clone();
                    move || same_tool(&current.get(), &tool).to_string()
                };
                view! {
                    <button aria-pressed=pressed title=format!("{} ({})", entry.label, entry.key)
                        on:click=move |_| state.update(|s| s.set_tool(tool.clone()))>
                        {entry.label}<span class="key">{entry.key.to_string()}</span>
                    </button>
                }
            })
            .collect_view()
    };

    view! {
        <header class="toolbar">
            <span class="title">{EN.app_title}</span>
            {buttons}
            <span class="sep"></span>
            <button disabled=move || !can_undo.get() on:click=move |_| { state.update(|s| { s.undo(now()); }); }>
                {EN.undo}
            </button>
            <button disabled=move || !can_redo.get() on:click=move |_| { state.update(|s| { s.redo(now()); }); }>
                {EN.redo}
            </button>
            <button disabled=move || !has_selection.get()
                on:click=move |_| state.update(|s| s.duplicate_selection(now()))>{EN.duplicate}</button>
            <button disabled=move || !has_selection.get()
                on:click=move |_| state.update(|s| s.delete_selection(now()))>{EN.delete}</button>
            <button on:click=move |_| state.update(|s| s.fit_to_content())>{EN.fit}</button>
        </header>
    }
}
