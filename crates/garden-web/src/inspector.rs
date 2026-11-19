//! Properties of the selected object.

use garden_core::geometry::SegmentKind;
use garden_core::model::{AreaType, Object, ObjectKind};
use garden_core::units::UnitSystem;
use garden_editor::inspector::{
    EdgeRow, Field, FieldError, apply_corner_angle, apply_edge_length, apply_field,
    apply_segment_kind, edge_rows, field_value, inspector_fields, measures,
};
use garden_editor::layers::{Align, ZOrder};
use garden_editor::render::style_class;
use garden_editor::state::EditorState;
use garden_editor::strings::{EN, category_label, kind_label};
use leptos::prelude::*;

use crate::app::now;
use crate::icons::{Icon, icon};
use crate::plan_art::plant_swatch;

fn field_label(f: Field) -> &'static str {
    match f {
        Field::Name => EN.field_name,
        Field::Width => EN.field_width,
        Field::Thickness => EN.field_thickness,
        Field::Height => EN.field_height,
        Field::Spread => EN.field_spread,
        Field::Text => EN.field_text,
    }
}

const AREA_TYPES: [AreaType; 6] = [
    AreaType::Bed,
    AreaType::Lawn,
    AreaType::Paving,
    AreaType::Gravel,
    AreaType::Water,
    AreaType::Other,
];

fn area_type_key(t: AreaType) -> &'static str {
    match t {
        AreaType::Bed => "bed",
        AreaType::Lawn => "lawn",
        AreaType::Paving => "paving",
        AreaType::Gravel => "gravel",
        AreaType::Water => "water",
        AreaType::Other => "other",
    }
}

fn commit(state: RwSignal<EditorState>, before: &Object, after: Object) {
    if &after != before {
        state.update(|s| {
            let _ = s.replace_object(after, now());
        });
    }
}

/// One text field. Invalid input shows a message and changes nothing.
#[component]
fn FieldInput(object: Object, field: Field, units: UnitSystem) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let initial = field_value(&object, field, units);
    let text = RwSignal::new(initial.clone());
    let error = RwSignal::new(None::<&'static str>);
    let object = StoredValue::new(object);
    let try_commit = move || {
        // The panel may already be gone (selection changed); then there is nothing to do.
        let (Some(input), Some(before)) = (text.try_get_untracked(), object.try_get_value()) else {
            return;
        };
        match apply_field(&before, field, &input, units) {
            Ok(after) => {
                error.set(None);
                commit(state, &before, after);
            }
            Err(e) => error.set(Some(e.message(&EN))),
        }
    };
    let id = format!("field-{field:?}");
    view! {
        <div class="field">
            <label for=id.clone()>{field_label(field)}</label>
            <input type="text" id=id prop:value=move || text.get() class:invalid=move || error.get().is_some()
                on:input=move |e| text.set(event_target_value(&e))
                on:keydown=move |e: leptos::ev::KeyboardEvent| match e.key().as_str() {
                    "Enter" => try_commit(),
                    "Escape" => {
                        text.set(initial.clone());
                        error.set(None);
                    }
                    _ => {}
                }
                on:blur=move |_| try_commit() />
            {move || error.get().map(|m| view! { <span class="error">{m}</span> })}
        </div>
    }
}

/// A small text box for one edge's length or one corner's angle. Invalid
/// input shows a message and changes nothing.
#[component]
fn EdgeField(
    object: Object,
    value: String,
    label: String,
    apply: Callback<(Object, String), Result<Object, FieldError>>,
) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let text = RwSignal::new(value.clone());
    let error = RwSignal::new(None::<&'static str>);
    let object = StoredValue::new(object);
    let try_commit = move || {
        let (Some(input), Some(before)) = (text.try_get_untracked(), object.try_get_value()) else {
            return;
        };
        match apply.run((before.clone(), input)) {
            Ok(after) => {
                error.set(None);
                commit(state, &before, after);
            }
            Err(e) => error.set(Some(e.message(&EN))),
        }
    };
    view! {
        <span class="edge-cell">
            <input type="text" aria-label=label prop:value=move || text.get()
                class:invalid=move || error.get().is_some()
                on:input=move |e| text.set(event_target_value(&e))
                on:keydown=move |e: leptos::ev::KeyboardEvent| match e.key().as_str() {
                    "Enter" => try_commit(),
                    "Escape" => {
                        text.set(value.clone());
                        error.set(None);
                    }
                    _ => {}
                }
                on:blur=move |_| try_commit() />
            {move || error.get().map(|m| view! { <span class="error">{m}</span> })}
        </span>
    }
}

fn segment_key(k: SegmentKind) -> &'static str {
    match k {
        SegmentKind::Line => "line",
        SegmentKind::Arc => "arc",
        SegmentKind::Cubic => "curve",
    }
}

/// Every edge of the outline: its length, whether it is straight, an arc or a
/// curve, and the corner angle after it.
fn edges_section(object: &Object, units: UnitSystem) -> Option<impl IntoView + use<>> {
    let rows = edge_rows(object, units);
    if rows.is_empty() {
        return None;
    }
    let state = expect_context::<RwSignal<EditorState>>();
    let closed = garden_editor::objects::kind_shape(&object.kind).is_some_and(|s| s.closed);
    let count = rows.len();
    let views = rows
        .into_iter()
        .map(|EdgeRow { index, length, kind, angle }| {
            let n = index + 1;
            let length_view = if kind == SegmentKind::Line {
                let apply = Callback::new(move |(o, t): (Object, String)| apply_edge_length(&o, index, &t, units));
                view! { <EdgeField object=object.clone() value=length label=format!("{} {n}", EN.edge) apply=apply /> }.into_any()
            } else {
                view! { <span class="edge-cell edge-static">{length}</span> }.into_any()
            };
            let obj = object.clone();
            let kinds = [(SegmentKind::Line, EN.straight), (SegmentKind::Arc, EN.arc), (SegmentKind::Cubic, EN.curve)]
                .into_iter()
                .map(|(k, label)| view! { <option value=segment_key(k) selected=k == kind>{label}</option> })
                .collect_view();
            let kind_view = view! {
                <select class="edge-kind" aria-label=format!("{} {n}", EN.edge) on:change=move |e| {
                    let key = event_target_value(&e);
                    let to = [SegmentKind::Line, SegmentKind::Arc, SegmentKind::Cubic]
                        .into_iter()
                        .find(|k| segment_key(*k) == key);
                    if let Some(to) = to
                        && let Ok(after) = apply_segment_kind(&obj, index, to)
                    {
                        commit(state, &obj, after);
                    }
                }>{kinds}</select>
            };
            let vertex = if closed { (index + 1) % count.max(1) } else { index + 1 };
            let angle_view = angle.map(|a| {
                let apply = Callback::new(move |(o, t): (Object, String)| apply_corner_angle(&o, vertex, &t));
                view! { <EdgeField object=object.clone() value=a label=format!("{} {}", EN.corner, n) apply=apply /> }
            });
            view! {
                <div class="edge-row">
                    <span class="edge-num">{n}</span>
                    {length_view}
                    {kind_view}
                    {angle_view}
                </div>
            }
        })
        .collect_view();
    Some(view! {
        <section class="inspector-section">
            <h3>{EN.edges}</h3>
            <div class="edge-head"><span></span><span>{EN.measure_length}</span><span>{EN.field_type}</span><span>{EN.corner}</span></div>
            {views}
        </section>
    })
}

/// Stacking order, alignment (two or more objects) and which layer the selection is on.
#[component]
fn Arrange(count: usize) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let z = move |to: ZOrder| state.update(|s| s.reorder_selection(to, now()));
    let al = move |how: Align| state.update(|s| s.align_selection(how, now()));
    let btn = |label: &'static str, ic: Icon, on: Box<dyn Fn() + Send + Sync>| {
        view! { <button class="icon-btn" data-tip=label aria-label=label on:click=move |_| on()>{icon(ic)}</button> }
    };
    let order = view! {
        <div class="icon-row">
            {btn(EN.bring_front, Icon::Front, Box::new(move || z(ZOrder::Front)))}
            {btn(EN.forward, Icon::Forward, Box::new(move || z(ZOrder::Forward)))}
            {btn(EN.backward, Icon::Backward, Box::new(move || z(ZOrder::Backward)))}
            {btn(EN.send_back, Icon::Back, Box::new(move || z(ZOrder::Back)))}
        </div>
    };
    let align = (count >= 2).then(|| {
        view! {
            <h3>{EN.align}</h3>
            <div class="icon-row">
                {btn(EN.align_left, Icon::AlignLeft, Box::new(move || al(Align::Left)))}
                {btn(EN.align_center_x, Icon::AlignCenterX, Box::new(move || al(Align::CenterX)))}
                {btn(EN.align_right, Icon::AlignRight, Box::new(move || al(Align::Right)))}
                {btn(EN.align_top, Icon::AlignTop, Box::new(move || al(Align::Top)))}
                {btn(EN.align_center_y, Icon::AlignCenterY, Box::new(move || al(Align::CenterY)))}
                {btn(EN.align_bottom, Icon::AlignBottom, Box::new(move || al(Align::Bottom)))}
            </div>
        }
    });
    let layers = Memo::new(move |_| {
        state.with(|s| {
            let current = s
                .selection
                .first()
                .and_then(|id| s.doc().object(*id))
                .map(|o| o.layer);
            let all: Vec<_> = s
                .doc()
                .layers
                .iter()
                .map(|l| (l.id, l.name.clone()))
                .collect();
            (all, current)
        })
    });
    let layer_select = move || {
        let (all, current) = layers.get();
        (all.len() > 1).then(|| {
            let options = all
                .iter()
                .map(|(id, name)| view! { <option value=id.0.to_string() selected=Some(*id) == current>{name.clone()}</option> })
                .collect_view();
            view! {
                <label class="layer-pick">
                    <span>{EN.move_to_layer}</span>
                    <select on:change=move |e| {
                        let v = event_target_value(&e);
                        if let Some((id, _)) = all.iter().find(|(id, _)| id.0.to_string() == v) {
                            let id = *id;
                            state.update(|s| s.move_selection_to_layer(id, now()));
                        }
                    }>{options}</select>
                </label>
            }
        })
    };
    view! {
        <section class="inspector-section">
            <h3>{EN.arrange}</h3>
            {order}
            {align}
            {layer_select}
        </section>
    }
}

/// Big illustration at the top of the card: the plant symbol, or a sample of the material.
fn hero(object: &Object) -> AnyView {
    match &object.kind {
        ObjectKind::Plant {
            symbol, category, ..
        } => view! { <div class="hero hero-plant">{plant_swatch(*symbol, *category)}</div> }
            .into_any(),
        kind => {
            let class = format!("hero hero-sample sample-{}", style_class(kind));
            view! { <div class=class></div> }.into_any()
        }
    }
}

#[component]
fn ObjectPanel(
    object: Object,
    units: UnitSystem,
    measures: Vec<(&'static str, String)>,
) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let subtitle = match &object.kind {
        ObjectKind::Plant { category, .. } => format!(
            "{} · {}",
            kind_label(&EN, &object.kind),
            category_label(&EN, *category)
        ),
        kind => kind_label(&EN, kind).to_string(),
    };
    let fields = inspector_fields(&object.kind);
    let name_field = fields
        .iter()
        .find(|f| **f == Field::Name)
        .map(|f| view! { <FieldInput object=object.clone() field=*f units=units /> });
    let tiles = fields
        .into_iter()
        .filter(|f| *f != Field::Name)
        .map(|f| view! { <FieldInput object=object.clone() field=f units=units /> })
        .collect_view();
    let stats = measures
        .into_iter()
        .map(|(label, value)| {
            view! {
                <div class="stat">
                    <span class="stat-label">{label}</span>
                    <span class="stat-value">{value}</span>
                </div>
            }
        })
        .collect_view();
    let area_select = match object.kind {
        ObjectKind::Area { area_type, .. } => {
            let obj = object.clone();
            let options = AREA_TYPES
                .iter()
                .map(|t| {
                    let label = kind_label(
                        &EN,
                        &ObjectKind::Area { area_type: *t, shape: garden_core::geometry::Shape::polyline(&[], true) },
                    );
                    view! { <option value=area_type_key(*t) selected=*t == area_type>{label}</option> }
                })
                .collect_view();
            Some(view! {
                <div class="field wide">
                    <label for="field-type">{EN.field_type}</label>
                    <select id="field-type" on:change=move |e| {
                        let key = event_target_value(&e);
                        if let Some(t) = AREA_TYPES.iter().find(|t| area_type_key(**t) == key) {
                            let mut after = obj.clone();
                            if let ObjectKind::Area { area_type, .. } = &mut after.kind {
                                *area_type = *t;
                            }
                            commit(state, &obj, after);
                        }
                    }>{options}</select>
                </div>
            })
        }
        _ => None,
    };
    let edges = edges_section(&object, units);
    let locked = object.locked;
    let obj = object.clone();
    view! {
        {hero(&object)}
        <div class="name-row">{name_field}</div>
        <p class="subtitle">{subtitle}</p>
        <div class="tiles">{tiles}{stats}{area_select}</div>
        <label class="lock">
            <input type="checkbox" checked=locked on:change=move |e| {
                let mut after = obj.clone();
                after.locked = event_target_checked(&e);
                commit(state, &obj, after);
            } />
            {EN.field_locked}
        </label>
        {edges}
        <Arrange count=1 />
    }
}

#[component]
pub fn Inspector() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    // Memos so typing isn't interrupted by unrelated state changes (e.g. pointer moves).
    let count = Memo::new(move |_| state.with(|s| s.selection.len()));
    let single = Memo::new(move |_| {
        state.with(|s| match s.selection.as_slice() {
            [id] => s.doc().object(*id).cloned().map(|o| {
                let m = measures(s.doc(), &o, s.doc().units);
                (o, m)
            }),
            _ => None,
        })
    });
    let units = Memo::new(move |_| state.with(|s| s.doc().units));
    move || {
        match (count.get(), single.get()) {
        (0, _) => None,
        (1, Some((object, m))) => Some(
            view! { <aside class="card inspector"><ObjectPanel object=object units=units.get() measures=m /></aside> }
                .into_any(),
        ),
        (n, _) => Some(view! {
            <aside class="card inspector small">
                <p class="multi-count">{format!("{n} {}", EN.n_selected)}</p>
                <Arrange count=n />
            </aside>
        }.into_any()),
    }
    }
}
