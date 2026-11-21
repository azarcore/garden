//! The catalog panel: search and browse plants and garden features.

use leptos::prelude::*;
use loma_core::model::{PlantCategory, PlantDef, PlantSymbol};
use loma_core::units::format_length;
use loma_editor::catalog::{
    EntryKind, FeatureId, Group, builtin_plants, catalog_with, feature_tool,
};
use loma_editor::custom_plants::{FormError, PlantForm, form_from_plant, plant_from_form};
use loma_editor::search::{recent_entries, remember, search};
use loma_editor::state::EditorState;
use loma_editor::strings::{EN, category_label};
use loma_editor::tools::ToolKind;
use wasm_bindgen::JsCast;

use crate::app::{LastPlant, now};
use crate::icons::{Icon, icon};
use crate::library::Library;
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

/// Close the catalog; it also shows while the plant tool is on, so that goes
/// back to the select tool.
pub fn close_catalog(open: RwSignal<bool>, state: RwSignal<EditorState>) {
    open.set(false);
    if state.with_untracked(|s| matches!(s.tool(), ToolKind::Plant(_))) {
        state.update(|s| s.switch_tool(ToolKind::Select, now()));
    }
}

/// Scroll the highlighted row into view (after the DOM has updated).
fn reveal_highlight() {
    request_animation_frame(|| {
        let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| {
            d.query_selector(".catalog .specimen.highlight")
                .ok()
                .flatten()
        }) else {
            return;
        };
        let opts = web_sys::ScrollIntoViewOptions::new();
        opts.set_block(web_sys::ScrollLogicalPosition::Nearest);
        el.scroll_into_view_with_scroll_into_view_options(&opts);
    });
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

/// Which part of the catalog is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Group(Group),
    Mine,
}

/// The New plant / Edit plant form while it is open.
#[derive(Clone, PartialEq)]
struct Editing {
    /// The plant being edited, or None for a new one.
    id: Option<String>,
    form: PlantForm,
    error: Option<FormError>,
    /// Saving to the browser failed (or isn't possible here).
    store_error: Option<&'static str>,
}

const KINDS: [PlantCategory; 10] = [
    PlantCategory::Tree,
    PlantCategory::FruitTree,
    PlantCategory::Shrub,
    PlantCategory::Perennial,
    PlantCategory::Annual,
    PlantCategory::Grass,
    PlantCategory::Climber,
    PlantCategory::Vegetable,
    PlantCategory::Herb,
    PlantCategory::Other,
];
const SYMBOLS: [PlantSymbol; 5] = [
    PlantSymbol::Round,
    PlantSymbol::Star,
    PlantSymbol::Spiky,
    PlantSymbol::Leafy,
    PlantSymbol::Conifer,
];

fn kind_key(c: PlantCategory) -> &'static str {
    match c {
        PlantCategory::Tree => "tree",
        PlantCategory::FruitTree => "fruit_tree",
        PlantCategory::Shrub => "shrub",
        PlantCategory::Perennial => "perennial",
        PlantCategory::Annual => "annual",
        PlantCategory::Grass => "grass",
        PlantCategory::Climber => "climber",
        PlantCategory::Vegetable => "vegetable",
        PlantCategory::Herb => "herb",
        PlantCategory::Other => "other",
    }
}

#[component]
pub fn CatalogPanel() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let lib = expect_context::<Library>();
    let LastPlant(last_plant) = expect_context::<LastPlant>();
    let CatalogOpen(open) = expect_context::<CatalogOpen>();
    let entries = Memo::new(move |_| lib.plants.with(|p| catalog_with(p)));
    let query = RwSignal::new(String::new());
    let filter = RwSignal::new(Filter::All);
    let recent = RwSignal::new(load_recent());
    let highlight = RwSignal::new(0usize);
    let editing = RwSignal::new(None::<Editing>);
    let confirm_delete = RwSignal::new(None::<String>);
    let plant_tool = Memo::new(move |_| state.with(|s| matches!(s.tool(), ToolKind::Plant(_))));
    let shown = move || open.get() || plant_tool.get();
    let units = Memo::new(move |_| state.with(|s| s.doc().units));

    // What's listed, in order: (optional heading, entry indices).
    let sections = Memo::new(move |_| {
        let q = query.get();
        let f = filter.get();
        let g = match f {
            Filter::Group(g) => Some(g),
            Filter::All | Filter::Mine => None,
        };
        entries.with(|all| {
            let keep = |i: &usize| match f {
                Filter::Mine => matches!(&all[*i].kind, EntryKind::Plant(p) if p.custom),
                _ => true,
            };
            let hits: Vec<usize> = search(all, &q, g).into_iter().filter(keep).collect();
            if !loma_editor::search::normalize(&q).is_empty() || f == Filter::Mine {
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
        let Some(entry) = entries.with_untracked(|all| all.get(i).cloned()) else {
            return;
        };
        recent.update(|r| *r = remember(r, &entry.id, RECENT_MAX));
        save_recent(&recent.get_untracked());
        highlight.set(0); // the list just changed (recent picks): start again at the top
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
    let close = move || close_catalog(open, state);
    let on_key = move |e: leptos::ev::KeyboardEvent| {
        let count = flat.with_untracked(Vec::len);
        match e.key().as_str() {
            "ArrowDown" if count > 0 => {
                e.prevent_default();
                highlight.update(|h| *h = (*h + 1).min(count - 1));
                reveal_highlight();
            }
            "ArrowUp" => {
                e.prevent_default();
                highlight.update(|h| *h = h.saturating_sub(1));
                reveal_highlight();
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
                    highlight.set(0);
                }
            }
            _ => {}
        }
    };

    // ── your own plants ─────────────────────────────────────────
    let start_new = move || {
        let form = PlantForm {
            name: query.get_untracked().trim().to_string(),
            ..PlantForm::default()
        };
        editing.set(Some(Editing {
            id: None,
            form,
            error: None,
            store_error: None,
        }));
    };
    let start_edit = move |p: PlantDef| {
        let form = form_from_plant(&p, units.get_untracked());
        editing.set(Some(Editing {
            id: Some(p.id),
            form,
            error: None,
            store_error: None,
        }));
    };
    let save_form = move || {
        let Some(ed) = editing.get_untracked() else {
            return;
        };
        match plant_from_form(&ed.form, ed.id.as_deref(), units.get_untracked()) {
            Err(e) => editing.update(|x| {
                if let Some(x) = x {
                    x.error = Some(e);
                }
            }),
            Ok(def) => {
                // only show the plant once the browser has really kept it
                leptos::task::spawn_local(async move {
                    let result = match lib.store.get_value() {
                        None => Err(EN.plant_no_storage),
                        Some(store) => store.put_plant(&def).await.map_err(|_| EN.plant_not_saved),
                    };
                    match result {
                        Err(message) => {
                            editing.try_update(|x| {
                                if let Some(x) = x {
                                    x.store_error = Some(message);
                                }
                            });
                        }
                        Ok(()) => {
                            lib.plants
                                .update(|l| match l.iter_mut().find(|p| p.id == def.id) {
                                    Some(existing) => *existing = def.clone(),
                                    None => l.push(def.clone()),
                                });
                            editing.try_set(None);
                            last_plant.set(def.clone());
                            state.update(|s| s.switch_tool(ToolKind::Plant(def), now()));
                        }
                    }
                });
            }
        }
    };
    let delete_error = RwSignal::new(None::<&'static str>);
    // after a plant is gone from storage: drop it from the library and the tool
    let forget_plant = move |id: String| {
        lib.plants.update(|l| l.retain(|p| p.id != id));
        // the plant tool must not keep placing a plant that no longer exists
        if last_plant.with_untracked(|l| l.id == id) {
            if let Some(fallback) = builtin_plants().into_iter().find(|p| p.id == "lavender") {
                last_plant.set(fallback);
            }
            if plant_tool.get_untracked() {
                state.update(|s| s.switch_tool(ToolKind::Select, now()));
            }
        }
    };
    let delete_plant = move |id: String| {
        leptos::task::spawn_local(async move {
            let deleted = match lib.store.get_value() {
                Some(store) => store.delete_plant(&id).await.is_ok(),
                None => false,
            };
            if !deleted {
                delete_error.try_set(Some(EN.plant_not_deleted));
                return;
            }
            confirm_delete.try_set(None);
            delete_error.try_set(None);
            forget_plant(id);
        });
    };

    // `pos` is the row's place in the visible list (an entry can be listed twice:
    // under Recently used and in its group)
    let row = move |pos: usize, i: usize| {
        let Some(entry) = entries.with_untracked(|all| all.get(i).cloned()) else {
            return ().into_any();
        };
        let is_hl = move || highlight.get() == pos;
        match &entry.kind {
            EntryKind::Plant(p) => {
                let id = p.id.clone();
                let selected =
                    move || (plant_tool.get() && last_plant.with(|l| l.id == id)).to_string();
                let spread = p.spread;
                let size = move || format_length(spread, units.get());
                let title = format!("{}: {} {}", p.name, size(), EN.plant_mature);
                let mine = p
                    .custom
                    .then(|| view! { <span class="mine-tag">{EN.mine}</span> });
                let button = view! {
                    <button class="specimen" aria-pressed=selected title=title class:highlight=is_hl
                        on:click=move |_| pick(i)>
                        {plant_swatch(p.symbol, p.category)}
                        <span class="specimen-name">{p.name.clone()}{mine}</span>
                        <span class="specimen-size">{size}</span>
                    </button>
                };
                if !p.custom {
                    return button.into_any();
                }
                let (edit_def, del_id, ask_id) = (p.clone(), p.id.clone(), p.id.clone());
                let asking = move || confirm_delete.get().as_deref() == Some(ask_id.as_str());
                view! {
                    <div class="specimen-wrap">
                        {button}
                        <div class="row-actions">
                            <button class="icon-btn" aria-label=EN.edit_plant data-tip=EN.edit_plant
                                on:click=move |_| start_edit(edit_def.clone())>{icon(Icon::Pencil)}</button>
                            <button class="icon-btn" aria-label=EN.delete_plant data-tip=EN.delete_plant
                                on:click={
                                    let id = p.id.clone();
                                    move |_| confirm_delete.set(Some(id.clone()))
                                }>{icon(Icon::Delete)}</button>
                        </div>
                        <Show when=asking>
                            <div class="confirm-delete" role="alertdialog">
                                <span>{EN.delete_plant_confirm}</span>
                                <button class="danger-text" on:click={
                                    let id = del_id.clone();
                                    move |_| delete_plant(id.clone())
                                }>{EN.delete_plant}</button>
                                <button on:click=move |_| {
                                    confirm_delete.set(None);
                                    delete_error.set(None);
                                }>{EN.cancel}</button>
                                {move || delete_error.get().map(|m| view! { <span class="error">{m}</span> })}
                            </div>
                        </Show>
                    </div>
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
        let chip = move |f: Filter, label: &'static str| {
            view! {
                <button class="chip" aria-pressed=move || (filter.get() == f).to_string()
                    on:click=move |_| { filter.set(f); highlight.set(0); }>{label}</button>
            }
        };
        let mut v = vec![chip(Filter::All, EN.all).into_any()];
        v.extend(
            Group::ALL
                .into_iter()
                .map(|g| chip(Filter::Group(g), g.label()).into_any()),
        );
        v.push(chip(Filter::Mine, EN.my_plants).into_any());
        v
    };

    let results = move || {
        let secs = sections.get();
        if secs.iter().all(|(_, ids)| ids.is_empty()) {
            let text = if filter.get() == Filter::Mine && query.get().trim().is_empty() {
                EN.no_my_plants
            } else {
                EN.no_matches
            };
            return view! { <p class="no-matches">{text}</p> }.into_any();
        }
        let mut start = 0;
        let mut views = Vec::new();
        for (heading, ids) in secs {
            let rows = ids
                .iter()
                .enumerate()
                .map(|(k, &i)| row(start + k, i))
                .collect_view();
            start += ids.len();
            views.push(view! {
                <section class="plant-group">
                    {heading.map(|h| view! { <h3>{h}</h3> })}
                    <div class="specimens">{rows}</div>
                </section>
            });
        }
        views.collect_view().into_any()
    };

    // the New plant / Edit plant form
    let field = move |set: fn(&mut PlantForm, String)| {
        move |e: leptos::ev::Event| {
            let v = event_target_value(&e);
            editing.update(|x| {
                if let Some(x) = x {
                    set(&mut x.form, v);
                    x.error = None;
                }
            });
        }
    };
    let value = move |get: fn(&PlantForm) -> String| {
        move || editing.with(|x| x.as_ref().map(|x| get(&x.form)).unwrap_or_default())
    };
    let error_for = move |which: &'static [FormError]| {
        move || {
            editing.with(|x| {
                x.as_ref()
                    .and_then(|x| x.error)
                    .filter(|e| which.contains(e))
                    .map(|e| view! { <span class="error">{e.message(&EN)}</span> })
            })
        }
    };
    let form_view = move || {
        let ed = editing.get()?;
        let heading = if ed.id.is_some() {
            EN.edit_plant
        } else {
            EN.new_plant
        };
        let category = ed.form.category;
        let kinds = KINDS
            .into_iter()
            .map(|k| {
                view! { <option value=kind_key(k) selected=k == category>{category_label(&EN, k)}</option> }
            })
            .collect_view();
        let symbols = SYMBOLS
            .into_iter()
            .map(|sym| {
                let chosen = move || {
                    editing.with(|x| x.as_ref().is_some_and(|x| x.form.symbol == sym)).to_string()
                };
                view! {
                    <button type="button" class="symbol-choice" aria-pressed=chosen
                        on:click=move |_| editing.update(|x| if let Some(x) = x { x.form.symbol = sym; })>
                        {plant_swatch(sym, category)}
                    </button>
                }
            })
            .collect_view();
        let placeholder = format_length(600.0, units.get_untracked());
        Some(view! {
            <form class="plant-form" on:submit=move |e| { e.prevent_default(); save_form(); }
                on:keydown=move |e: leptos::ev::KeyboardEvent| if e.key() == "Escape" {
                    e.prevent_default();
                    editing.set(None);
                }>
                <h3>{heading}</h3>
                <label class="form-field">
                    <span>{EN.plant_name}</span>
                    <input type="text" prop:value=value(|f| f.name.clone()) on:input=field(|f, v| f.name = v) />
                    {error_for(&[FormError::NameMissing, FormError::NameTooLong])}
                </label>
                <label class="form-field">
                    <span>{EN.plant_kind}</span>
                    <select on:change=move |e| {
                        let key = event_target_value(&e);
                        if let Some(k) = KINDS.into_iter().find(|k| kind_key(*k) == key) {
                            editing.update(|x| if let Some(x) = x { x.form.category = k; });
                        }
                    }>{kinds}</select>
                </label>
                <div class="form-field">
                    <span>{EN.plant_symbol}</span>
                    <div class="symbol-choices" role="group">{symbols}</div>
                </div>
                <div class="form-row">
                    <label class="form-field">
                        <span>{EN.plant_spread}</span>
                        <input type="text" placeholder=placeholder.clone() prop:value=value(|f| f.spread.clone())
                            on:input=field(|f, v| f.spread = v) />
                        {error_for(&[FormError::SpreadInvalid])}
                    </label>
                    <label class="form-field">
                        <span>{EN.plant_height}</span>
                        <input type="text" placeholder=placeholder prop:value=value(|f| f.height.clone())
                            on:input=field(|f, v| f.height = v) />
                        {error_for(&[FormError::HeightInvalid])}
                    </label>
                </div>
                <label class="form-field">
                    <span>{EN.plant_aliases}</span>
                    <input type="text" prop:value=value(|f| f.aliases.clone()) on:input=field(|f, v| f.aliases = v) />
                </label>
                {move || editing.with(|x| x.as_ref().and_then(|x| x.store_error))
                    .map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
                <div class="form-actions">
                    <button type="submit" class="trace-primary">{EN.save}</button>
                    <button type="button" on:click=move |_| editing.set(None)>{EN.cancel}</button>
                </div>
            </form>
        })
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
                <Show when=move || editing.with(Option::is_none)
                    fallback=move || view! { <div class="catalog-results">{form_view}</div> }>
                    <input id=SEARCH_ID class="catalog-search" type="search" placeholder=EN.search_catalog
                        aria-label=EN.search_catalog autocomplete="off" prop:value=move || query.get()
                        on:input=move |e| { query.set(event_target_value(&e)); highlight.set(0); }
                        on:keydown=on_key />
                    <div class="chips" role="group">{chips}</div>
                    <button class="new-plant" on:click=move |_| start_new()>
                        {icon(Icon::Plus)}{EN.new_plant}
                    </button>
                    <div class="catalog-results">{results}</div>
                </Show>
            </aside>
        </Show>
    }
}
