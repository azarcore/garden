//! Tool rail (left) and top bar (document actions, units, snapping).

use garden_core::model::{AreaType, LinearKind, ObjectKind, PlantDef, StructureKind};
use garden_core::units::{UnitSystem, format_length};
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use garden_editor::tools::{DrawTarget, Input, Key, LineTarget, Modifiers, ToolKind, hint};
use leptos::prelude::*;

use crate::app::{LastPlant, now};
use crate::icons::{Icon, icon};
use crate::library::{DesignsOpen, Library, SaveStatus};

/// A tool: label, keyboard shortcut, icon, and the group it sits in.
pub struct ToolEntry {
    pub label: &'static str,
    pub key: char,
    pub icon: Icon,
    pub group: u8,
    pub tool: ToolKind,
}

/// Every tool, in rail order. Groups: 0 select, 1 ground, 2 lines, 3 things.
pub fn tool_entries(plant: PlantDef) -> Vec<ToolEntry> {
    let e = |label, key, icon, group, tool| ToolEntry {
        label,
        key,
        icon,
        group,
        tool,
    };
    let line = |kind, thickness, height| {
        ToolKind::Polyline(LineTarget::Linear {
            kind,
            thickness,
            height,
        })
    };
    vec![
        e(EN.tool_select, 'V', Icon::Select, 0, ToolKind::Select),
        e(
            EN.tool_plot,
            'K',
            Icon::Plot,
            1,
            ToolKind::Polygon(DrawTarget::Plot),
        ),
        e(
            EN.tool_bed,
            'B',
            Icon::Bed,
            1,
            ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)),
        ),
        e(
            EN.tool_bed_rect,
            'R',
            Icon::RectBed,
            1,
            ToolKind::Rect(DrawTarget::Area(AreaType::Bed)),
        ),
        e(
            EN.tool_lawn,
            'E',
            Icon::Lawn,
            1,
            ToolKind::Ellipse(DrawTarget::Area(AreaType::Lawn)),
        ),
        e(
            EN.tool_paving,
            'A',
            Icon::Paving,
            1,
            ToolKind::Rect(DrawTarget::Area(AreaType::Paving)),
        ),
        e(
            EN.tool_gravel,
            'X',
            Icon::Gravel,
            1,
            ToolKind::Polygon(DrawTarget::Area(AreaType::Gravel)),
        ),
        e(
            EN.tool_water,
            'O',
            Icon::Water,
            1,
            ToolKind::Ellipse(DrawTarget::Area(AreaType::Water)),
        ),
        e(
            EN.tool_path,
            'P',
            Icon::Path,
            2,
            ToolKind::Polyline(LineTarget::Path { width: 900.0 }),
        ),
        e(
            EN.tool_fence,
            'F',
            Icon::Fence,
            2,
            line(LinearKind::Fence, 50.0, 1800.0),
        ),
        e(
            EN.tool_wall,
            'W',
            Icon::Wall,
            2,
            line(LinearKind::Wall, 200.0, 1800.0),
        ),
        e(
            EN.tool_hedge,
            'H',
            Icon::Hedge,
            2,
            line(LinearKind::Hedge, 600.0, 1500.0),
        ),
        e(
            EN.tool_structure,
            'S',
            Icon::Structure,
            3,
            ToolKind::Rect(DrawTarget::Structure(StructureKind::Shed)),
        ),
        e(EN.tool_plant, 'G', Icon::Plant, 3, ToolKind::Plant(plant)),
        e(EN.tool_text, 'T', Icon::Text, 3, ToolKind::Text),
        e(
            EN.tool_dimension,
            'D',
            Icon::Dimension,
            3,
            ToolKind::Dimension,
        ),
        e(EN.tool_measure, 'M', Icon::Measure, 3, ToolKind::Measure),
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
pub fn ToolDock() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let LastPlant(last_plant) = expect_context::<LastPlant>();
    let current = Memo::new(move |_| state.with(|s| s.tool().clone()));
    let buttons = move || {
        let mut last_group = 0;
        tool_entries(last_plant.get())
            .into_iter()
            .map(|entry| {
                let divider = (entry.group != last_group).then(|| view! { <span class="dock-sep"></span> });
                last_group = entry.group;
                let tool = entry.tool.clone();
                let is_plant = matches!(tool, ToolKind::Plant(_));
                let pressed = {
                    let tool = tool.clone();
                    move || same_tool(&current.get(), &tool).to_string()
                };
                view! {
                    {divider}
                    <button class="dock-tool" class:plant-tool=is_plant aria-pressed=pressed aria-label=entry.label
                        data-tip=format!("{}  {}", entry.label, entry.key)
                        on:click=move |_| state.update(|s| s.switch_tool(tool.clone(), now()))>
                        {icon(entry.icon)}
                    </button>
                }
            })
            .collect_view()
    };
    let crate::catalog::CatalogOpen(catalog_open) = expect_context::<crate::catalog::CatalogOpen>();
    // the panel also shows while the plant tool is on
    let catalog_shown =
        move || catalog_open.get() || state.with(|s| matches!(s.tool(), ToolKind::Plant(_)));
    view! {
        <nav class="card dock" aria-label=EN.tools_label>
            <button class="dock-tool" aria-pressed=move || catalog_shown().to_string() aria-label=EN.catalog
                data-tip=format!("{}  /", EN.catalog)
                on:click=move |_| {
                    if catalog_shown() {
                        crate::catalog::close_catalog(catalog_open, state);
                    } else {
                        crate::catalog::focus_search(catalog_open);
                    }
                }>
                {icon(Icon::Search)}
            </button>
            <span class="dock-sep"></span>
            {buttons}
        </nav>
    }
}

/// What to do next with the current tool, plus Finish / Undo point / Cancel
/// while a line or shape is being drawn (so no keyboard is needed).
#[component]
pub fn HintBar() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let info = Memo::new(move |_| {
        state.with(|s| {
            let text = if s.node_mode() {
                EN.hint_node
            } else {
                hint(
                    s.tool(),
                    s.points_placed(),
                    s.doc().background.is_some(),
                    &EN,
                )
            };
            (text, s.points_placed())
        })
    });
    // Finish and Undo point only make sense while a line or shape is being drawn.
    let line_tool = Memo::new(move |_| {
        state.with(|s| matches!(s.tool(), ToolKind::Polygon(_) | ToolKind::Polyline(_)))
    });
    let two_point_tool = Memo::new(move |_| {
        state.with(|s| matches!(s.tool(), ToolKind::Dimension | ToolKind::Measure))
    });
    let key = move |k: Key| state.update(|s| s.handle(Input::Key(k, Modifiers::default()), now()));
    // Set scale: after two clicks, type the real distance here.
    let scaling = Memo::new(move |_| state.with(|s| s.pending_scale().is_some()));
    let distance = RwSignal::new(String::new());
    let scale_error = RwSignal::new(None::<&'static str>);
    let apply = move || {
        let text = distance.get_untracked();
        let mut result = Ok(());
        state.update(|s| result = s.apply_scale(&text, now()));
        match result {
            Ok(()) => {
                scale_error.set(None);
                distance.set(String::new());
            }
            Err(e) => scale_error.set(Some(e.message(&EN))),
        }
    };
    view! {
        <div class="hint-bar" role="status"
            class:idle=move || state.with(|s| matches!(s.tool(), ToolKind::Select) && !s.node_mode())>
            <span class="hint-text">{move || info.get().0}</span>
            <Show when=move || scaling.get()>
                <input class="hint-input" type="text" placeholder=EN.scale_placeholder prop:value=move || distance.get()
                    on:input=move |e| distance.set(event_target_value(&e))
                    on:keydown=move |e: leptos::ev::KeyboardEvent| if e.key() == "Enter" { apply() } />
                <button class="hint-btn primary" on:click=move |_| apply()>{EN.apply}</button>
                <button class="hint-btn" on:click=move |_| key(Key::Escape)>{EN.cancel}</button>
                {move || scale_error.get().map(|m| view! { <span class="hint-error">{m}</span> })}
            </Show>
            <Show when=move || { info.get().1 > 0 && two_point_tool.get() }>
                <button class="hint-btn" on:click=move |_| key(Key::Escape)>{EN.cancel}</button>
            </Show>
            <Show when=move || { info.get().1 > 0 && line_tool.get() }>
                <button class="hint-btn primary" on:click=move |_| key(Key::Enter)>{EN.finish}</button>
                <button class="hint-btn" on:click=move |_| state.update(|s| { s.undo(now()); })>{EN.undo_point}</button>
                <button class="hint-btn" on:click=move |_| key(Key::Escape)>{EN.cancel}</button>
            </Show>
        </div>
    }
}

/// Top-left: what this design is.
#[component]
pub fn TitleCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let name = Memo::new(move |_| state.with(|s| s.doc().name.clone()));
    let plot = Memo::new(move |_| {
        state.with(|s| {
            let units = s.doc().units;
            s.doc().objects.iter().find_map(|o| match &o.kind {
                ObjectKind::PlotBoundary { shape } => {
                    let b = shape.bounding_box();
                    Some(format!(
                        "{} × {}",
                        format_length(b.width(), units),
                        format_length(b.height(), units)
                    ))
                }
                _ => None,
            })
        })
    });
    let lib = expect_context::<Library>();
    let DesignsOpen(designs_open) = expect_context::<DesignsOpen>();
    let status_key = move || match lib.status.get() {
        SaveStatus::Saved => "saved",
        SaveStatus::Saving => "saving",
        SaveStatus::Unsaved => "unsaved",
        SaveStatus::Unavailable => "unavailable",
    };
    let status_text = move || match lib.status.get() {
        SaveStatus::Saved => EN.saved,
        SaveStatus::Saving => EN.saving,
        SaveStatus::Unsaved => EN.save_failed,
        SaveStatus::Unavailable => EN.not_saved,
    };
    view! {
        <button class="card title-card" aria-expanded=move || designs_open.get().to_string()
            aria-label=EN.my_designs on:click=move |_| designs_open.update(|o| *o = !*o)>
            <span class="logo">{icon(Icon::Leaf)}</span>
            <div class="title-text">
                <strong>{move || name.get()}</strong>
                <span>{move || plot.get().unwrap_or_else(|| EN.no_plot_yet.to_string())}</span>
            </div>
            <span class="save-status" data-status=status_key title=status_text>{status_text}</span>
            <span class="title-chevron" aria-hidden="true"></span>
        </button>
    }
}

/// Top-right: history, editing actions, units and snapping.
#[component]
pub fn ControlsCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let can_undo = Memo::new(move |_| state.with(|s| s.can_undo()));
    let can_redo = Memo::new(move |_| state.with(|s| s.can_redo()));
    let has_selection = Memo::new(move |_| state.with(|s| !s.selection.is_empty()));
    let units = Memo::new(move |_| state.with(|s| s.doc().units));
    let snap_grid = Memo::new(move |_| state.with(|s| s.snap_grid));
    let snap_objects = Memo::new(move |_| state.with(|s| s.snap_objects));
    let set_units = move |u: UnitSystem| state.update(|s| s.set_units(u, now()));
    let trace = expect_context::<crate::trace::Trace>();
    let crate::print::PrintOpen(print_open) = expect_context::<crate::print::PrintOpen>();
    let crate::list::ListOpen(list_open) = expect_context::<crate::list::ListOpen>();
    let crate::layers::LayersOpen(layers_open) = expect_context::<crate::layers::LayersOpen>();
    let view3d = expect_context::<crate::view3d::View3d>();
    let care = expect_context::<crate::care::CareUi>();
    let design = Memo::new(move |_| state.with(|s| s.mode() == garden_editor::state::Mode::Design));

    view! {
            <div class="card controls" role="toolbar" aria-label=EN.actions_label>
                <crate::garden::ModeSwitch />
                <span class="card-sep"></span>
                <button class="icon-btn" data-tip=EN.undo aria-label=EN.undo disabled=move || !can_undo.get()
                    on:click=move |_| state.update(|s| { s.undo(now()); })>{icon(Icon::Undo)}</button>
                <button class="icon-btn" data-tip=EN.redo aria-label=EN.redo disabled=move || !can_redo.get()
                    on:click=move |_| state.update(|s| { s.redo(now()); })>{icon(Icon::Redo)}</button>
                <Show when=move || design.get()>
                <span class="card-sep"></span>
                <button class="icon-btn" data-tip=EN.duplicate aria-label=EN.duplicate disabled=move || !has_selection.get()
                    on:click=move |_| state.update(|s| s.duplicate_selection(now()))>{icon(Icon::Duplicate)}</button>
                <button class="icon-btn" data-tip=EN.delete aria-label=EN.delete disabled=move || !has_selection.get()
                    on:click=move |_| state.update(|s| s.delete_selection(now()))>{icon(Icon::Delete)}</button>
                </Show>
                <button class="icon-btn" data-tip=EN.fit aria-label=EN.fit
                    on:click=move |_| state.update(|s| s.fit_to_content())>{icon(Icon::Fit)}</button>
                <Show when=move || design.get()>
                <span class="card-sep"></span>
                <button class="icon-btn toggle" data-tip=EN.snap_grid aria-label=EN.snap_grid
                    aria-pressed=move || snap_grid.get().to_string()
                    on:click=move |_| state.update(|s| s.snap_grid = !s.snap_grid)>{icon(Icon::Grid)}</button>
                <button class="icon-btn toggle" data-tip=EN.snap_objects aria-label=EN.snap_objects
                    aria-pressed=move || snap_objects.get().to_string()
                    on:click=move |_| state.update(|s| s.snap_objects = !s.snap_objects)>{icon(Icon::Magnet)}</button>
                <span class="card-sep"></span>
    <button class="icon-btn toggle" data-tip=EN.tool_trace aria-label=EN.tool_trace
                    aria-pressed=move || trace.open.get().to_string()
                    on:click=move |_| trace.open.update(|o| *o = !*o)>{icon(Icon::Photo)}</button>
                </Show>
            <button class="icon-btn toggle" data-tip=EN.print aria-label=EN.print
                aria-pressed=move || print_open.get().to_string()
                on:click=move |_| print_open.update(|o| *o = !*o)>{icon(Icon::Printer)}</button>
            <Show when=move || !design.get()>
                <button class="icon-btn" data-tip=EN.week_title aria-label=EN.week_title
                    aria-pressed=move || care.week_open.get().to_string()
                    on:click=move |_| care.week_open.update(|o| *o = !*o)>{icon(Icon::Calendar)}</button>
                <button class="icon-btn" data-tip=EN.products_button aria-label=EN.products_button
                    aria-pressed=move || care.products_open.get().to_string()
                    on:click=move |_| care.products_open.update(|o| *o = !*o)>{icon(Icon::Bottle)}</button>
            </Show>
            // not a .toggle: the list stays reachable on phones (at the garden centre)
            <button class="icon-btn" data-tip=EN.list_title aria-label=EN.list_title
                aria-pressed=move || list_open.get().to_string()
                on:click=move |_| list_open.update(|o| *o = !*o)>{icon(Icon::List)}</button>
            <Show when=move || design.get()>
            <button class="icon-btn toggle" data-tip=EN.layers aria-label=EN.layers
                aria-pressed=move || layers_open.get().to_string()
                on:click=move |_| layers_open.update(|o| *o = !*o)>{icon(Icon::Layers)}</button>
            </Show>
            <button class="icon-btn" data-tip=format!("{}  3", EN.view_3d) aria-label=EN.view_3d
                on:click=move |_| crate::view3d::open_3d(view3d, state)>{icon(Icon::Cube)}</button>
                <Show when=move || design.get()>
                <span class="card-sep"></span>
                            <div class="segmented" role="group">
                    <button aria-pressed=move || (units.get() == UnitSystem::Metric).to_string()
                        on:click=move |_| set_units(UnitSystem::Metric)>{EN.units_metric}</button>
                    <button aria-pressed=move || (units.get() == UnitSystem::Imperial).to_string()
                        on:click=move |_| set_units(UnitSystem::Imperial)>{EN.units_imperial}</button>
                </div>
                </Show>
            </div>
        }
}
