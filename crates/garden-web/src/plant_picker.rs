//! Plant catalog flyout, shown beside the tool rail while the plant tool is active.

use garden_core::model::PlantCategory;
use garden_core::units::format_length;
use garden_editor::catalog::builtin_plants;
use garden_editor::state::EditorState;
use garden_editor::strings::{EN, category_label};
use garden_editor::tools::ToolKind;
use leptos::prelude::*;

use crate::app::{LastPlant, now};
use crate::plan_art::plant_swatch;

#[component]
pub fn PlantPicker() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let LastPlant(last_plant) = expect_context::<LastPlant>();
    let active = Memo::new(move |_| state.with(|s| matches!(s.tool(), ToolKind::Plant(_))));
    let units = Memo::new(move |_| state.with(|s| s.doc().units));

    let list = move || {
        let plants = builtin_plants();
        let mut categories: Vec<PlantCategory> = Vec::new();
        for p in &plants {
            if !categories.contains(&p.category) {
                categories.push(p.category);
            }
        }
        categories
            .into_iter()
            .map(|cat| {
                let rows = plants
                    .iter()
                    .filter(|p| p.category == cat)
                    .cloned()
                    .map(|p| {
                        let id = p.id.clone();
                        let pressed = move || last_plant.with(|l| l.id == id).to_string();
                        let size = format_length(p.spread, units.get());
                        let name = p.name.clone();
                        let swatch = plant_swatch(p.symbol, p.category);
                        let title = format!("{name}: {size} {}", EN.plant_mature);
                        view! {
                            <button class="specimen" aria-pressed=pressed
                                title=title
                                on:click=move |_| {
                                    last_plant.set(p.clone());
                                    state.update(|s| s.switch_tool(ToolKind::Plant(p.clone()), now()));
                                }>
                                {swatch}
                                <span class="specimen-name">{name.clone()}</span>
                                <span class="specimen-size">{size}</span>
                            </button>
                        }
                    })
                    .collect_view();
                view! {
                    <section class="plant-group">
                        <h3>{category_label(&EN, cat)}</h3>
                        <div class="specimens">{rows}</div>
                    </section>
                }
            })
            .collect_view()
    };

    view! {
        <Show when=move || active.get()>
            <aside class="card picker" aria-label=EN.plants_heading>
                <h2>{EN.plants_heading}</h2>
                {list}
            </aside>
        </Show>
    }
}
