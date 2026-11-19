//! Properties of the selected object.

use garden_core::model::{AreaType, Object, ObjectKind};
use garden_core::units::UnitSystem;
use garden_editor::inspector::{Field, apply_field, field_value, inspector_fields, measures};
use garden_editor::render::style_class;
use garden_editor::state::EditorState;
use garden_editor::strings::{EN, category_label, kind_label};
use leptos::prelude::*;

use crate::app::now;
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
        (n, _) => Some(view! { <aside class="card inspector small">{format!("{n} {}", EN.n_selected)}</aside> }.into_any()),
    }
    }
}
