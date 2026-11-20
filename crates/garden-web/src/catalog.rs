//! The catalog panel: search and browse plants and garden features.

use garden_core::units::format_length;
use garden_editor::catalog::{CatalogEntry, EntryKind, FeatureId, Group, catalog, feature_tool};
use garden_editor::search::{recent_entries, remember, search};
use garden_editor::state::EditorState;
use garden_editor::strings::EN;
use garden_editor::tools::ToolKind;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::app::{LastPlant, now};
use crate::icons::{Icon, icon};
use crate::plan_art::plant_swatch;

/// Whether the catalog panel was opened (it also shows while the plant tool is on).
#[derive(Clone, Copy)]
pub struct CatalogOpen(pub RwSignal<bool>);

const RECENT_KEY: &str = "garden.recent";
const RECENT_MAX: usize = 6;
const SEARCH_ID: &str = "catalog-search";

/// Recently used ids from this browser (empty if storage is unavailable).
fn load_recent() -> Vec<String> {
    let text = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item(RECENT_KEY).ok().flatten());
    text.and_then(|t| js_sys::JSON::parse(&t).ok())
        .and_then(|v| v.dyn_into::<js_sys::Array>().ok())
        .map(|a| a.iter().filter_map(|v| v.as_string()).collect())
        .unwrap_or_default()
}

fn save_recent(ids: &[String]) {
    let array: js_sys::Array = ids
        .iter()
        .map(|s| wasm_bindgen::JsValue::from_str(s))
        .collect();
    if let (Some(storage), Ok(text)) = (
        web_sys::window().and_then(|w| w.local_storage().ok().flatten()),
        js_sys::JSON::stringify(&array),
    ) {
        let _ = storage.set_item(RECENT_KEY, &String::from(text));
    }
}

fn feature_icon(f: FeatureId) -> Icon {
    match f {
        FeatureId::Bed => Icon::Bed,
        FeatureId::Lawn => Icon::Lawn,
        FeatureId::Paving => Icon::Paving,
        FeatureId::Gravel => Icon::Gravel,
        FeatureId::Water => Icon::Water,
        FeatureId::Path => Icon::Path,
        FeatureId::Fence => Icon::Fence,
        FeatureId::Wall => Icon::Wall,
        FeatureId::Hedge => Icon::Hedge,
        FeatureId::RaisedBed => Icon::RectBed,
        FeatureId::Shed | FeatureId::Greenhouse | FeatureId::Pergola | FeatureId::Seating => {
            Icon::Structure
        }
    }
}

/// Open the catalog and put the cursor in its search box.
pub fn focus_search(open: RwSignal<bool>) {
    open.set(true);
    request_animation_frame(|| {
        if let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(SEARCH_ID))
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            let _ = el.focus();
            el.select();
        }
    });
}

#[component]
pub fn CatalogPanel() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let LastPlant(last_plant) = expect_context::<LastPlant>();
    let CatalogOpen(open) = expect_context::<CatalogOpen>();
    let entries = StoredValue::new(catalog());
    let query = RwSignal::new(String::new());
    let group = RwSignal::new(None::<Group>);
    let recent = RwSignal::new(load_recent());
    let highlight = RwSignal::new(0usize);
    let plant_tool = Memo::new(move |_| state.with(|s| matches!(s.tool(), ToolKind::Plant(_))));
    let shown = move || open.get() || plant_tool.get();
    let units = Memo::new(move |_| state.with(|s| s.doc().units));

    // What's listed, in order: (optional heading, entry indices).
    let sections = Memo::new(move |_| {
        let q = query.get();
        let g = group.get();
        entries.with_value(|all| {
            let hits = search(all, &q, g);
            if !garden_editor::search::normalize(&q).is_empty() {
                return vec![(None, hits)];
            }
            let mut out = Vec::new();
            let recents: Vec<usize> = recent_entries(all, &recent.get())
                .into_iter()
                .filter(|i| g.is_none_or(|g| all[*i].group == g))
                .collect();
            if !recents.is_empty() {
                out.push((Some(EN.recently_used), recents));
            }
            for grp in Group::ALL.into_iter().filter(|x| g.is_none_or(|g| *x == g)) {
                let ids: Vec<usize> = hits
                    .iter()
                    .copied()
                    .filter(|i| all[*i].group == grp)
                    .collect();
                if !ids.is_empty() {
                    out.push((Some(grp.label()), ids));
                }
            }
            out
        })
    });
    let flat = Memo::new(move |_| {
        sections.with(|s| {
            s.iter()
                .flat_map(|(_, ids)| ids.clone())
                .collect::<Vec<_>>()
        })
    });

    let pick = move |i: usize| {
        let entry: CatalogEntry = entries.with_value(|all| all[i].clone());
        recent.update(|r| *r = remember(r, &entry.id, RECENT_MAX));
        save_recent(&recent.get_untracked());
        match entry.kind {
            EntryKind::Plant(def) => {
                last_plant.set(def.clone());
                state.update(|s| s.switch_tool(ToolKind::Plant(def), now()));
            }
            EntryKind::Feature(f) => {
                open.set(false);
                state.update(|s| s.switch_tool(feature_tool(f), now()));
            }
        }
    };
    let close = move || {
        open.set(false);
        if plant_tool.get_untracked() {
            state.update(|s| s.switch_tool(ToolKind::Select, now()));
        }
    };
    let on_key = move |e: leptos::ev::KeyboardEvent| {
        let count = flat.with_untracked(Vec::len);
        match e.key().as_str() {
            "ArrowDown" if count > 0 => {
                e.prevent_default();
                highlight.update(|h| *h = (*h + 1).min(count - 1));
            }
            "ArrowUp" => {
                e.prevent_default();
                highlight.update(|h| *h = h.saturating_sub(1));
            }
            "Enter" => {
                if let Some(i) = flat.with_untracked(|f| f.get(highlight.get_untracked()).copied())
                {
                    pick(i);
                }
            }
            "Escape" => {
                if query.get_untracked().is_empty() {
                    close();
                } else {
                    query.set(String::new());
                }
            }
            _ => {}
        }
    };

    let row = move |i: usize| {
        let entry = entries.with_value(|all| all[i].clone());
        let is_hl = move || flat.with(|f| f.get(highlight.get()) == Some(&i));
        match &entry.kind {
            EntryKind::Plant(p) => {
                let id = p.id.clone();
                let selected =
                    move || (plant_tool.get() && last_plant.with(|l| l.id == id)).to_string();
                let size = format_length(p.spread, units.get_untracked());
                let title = format!("{}: {size} {}", p.name, EN.plant_mature);
                view! {
                    <button class="specimen" aria-pressed=selected title=title class:highlight=is_hl
                        on:click=move |_| pick(i)>
                        {plant_swatch(p.symbol, p.category)}
                        <span class="specimen-name">{p.name.clone()}</span>
                        <span class="specimen-size">{size}</span>
                    </button>
                }
                .into_any()
            }
            EntryKind::Feature(f) => view! {
                <button class="specimen feature" class:highlight=is_hl on:click=move |_| pick(i)>
                    <span class="feature-icon">{icon(feature_icon(*f))}</span>
                    <span class="specimen-name">{entry.name.clone()}</span>
                </button>
            }
            .into_any(),
        }
    };

    let chips = move || {
        let chip = move |g: Option<Group>, label: &'static str| {
            view! {
                <button class="chip" aria-pressed=move || (group.get() == g).to_string()
                    on:click=move |_| { group.set(g); highlight.set(0); }>{label}</button>
            }
        };
        let mut v = vec![chip(None, EN.all).into_any()];
        v.extend(
            Group::ALL
                .into_iter()
                .map(|g| chip(Some(g), g.label()).into_any()),
        );
        v
    };

    view! {
        <Show when=shown>
            <aside class="card picker catalog" aria-label=EN.catalog>
                <header class="catalog-head">
                    <h2>{EN.catalog}</h2>
                    <button class="icon-btn" aria-label=EN.close on:click=move |_| close()>
                        {icon(Icon::Close)}
                    </button>
                </header>
                <input id=SEARCH_ID class="catalog-search" type="search" placeholder=EN.search_catalog
                    aria-label=EN.search_catalog autocomplete="off" prop:value=move || query.get()
                    on:input=move |e| { query.set(event_target_value(&e)); highlight.set(0); }
                    on:keydown=on_key />
                <div class="chips" role="group">{chips}</div>
                {move || {
                    let secs = sections.get();
                    if secs.iter().all(|(_, ids)| ids.is_empty()) {
                        return view! { <p class="no-matches">{EN.no_matches}</p> }.into_any();
                    }
                    secs.into_iter()
                        .map(|(heading, ids)| view! {
                            <section class="plant-group">
                                {heading.map(|h| view! { <h3>{h}</h3> })}
                                <div class="specimens">{ids.into_iter().map(row).collect_view()}</div>
                            </section>
                        })
                        .collect_view()
                        .into_any()
                }}
            </aside>
        </Show>
    }
}
