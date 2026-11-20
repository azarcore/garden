//! Garden mode cards: the plant card (one or several plants), the job form
//! and the products library.

use garden_core::date::Date;
use garden_core::model::{
    AmountUnit, Health, Job, LogEntry, LogId, ObjectId, ObjectKind, Product, ProductId, ProductKind,
};
use garden_core::units::format_length;
use garden_editor::care::{
    date_label, entry_warnings, group_safe_to_harvest, history, not_before, plant_name,
    safe_to_harvest, season_count, warning_text,
};
use garden_editor::garden::{LogDraft, ProductDraft, parse_days};
use garden_editor::list::parse_price;
use garden_editor::state::{EditorState, Mode};
use garden_editor::strings::{
    EN, amount_unit_label, health_label, job_label, kind_label, product_kind_label,
};
use leptos::prelude::*;

use crate::app::now;
use crate::garden::{SelectSeveral, today};
use crate::icons::{Icon, icon};

const HEALTHS: [Health; 4] = [Health::Healthy, Health::Watch, Health::Sick, Health::Gone];
const JOBS: [Job; 7] = [
    Job::Sprayed,
    Job::Fed,
    Job::Pruned,
    Job::Watered,
    Job::Harvested,
    Job::Planted,
    Job::Other,
];
const UNITS: [AmountUnit; 6] = [
    AmountUnit::Kg,
    AmountUnit::G,
    AmountUnit::Lb,
    AmountUnit::Pieces,
    AmountUnit::L,
    AmountUnit::Ml,
];
const KINDS: [ProductKind; 5] = [
    ProductKind::Fungicide,
    ProductKind::Insecticide,
    ProductKind::Herbicide,
    ProductKind::Fertiliser,
    ProductKind::Other,
];

/// What the job form starts with.
#[derive(Clone, Debug, PartialEq)]
pub struct LogSeed {
    id: Option<LogId>,
    plants: Vec<ObjectId>,
    job: Job,
    date: Date,
    product: Option<ProductId>,
    amount: String,
    unit: AmountUnit,
    note: String,
}

impl LogSeed {
    fn new(plants: Vec<ObjectId>) -> LogSeed {
        LogSeed {
            id: None,
            plants,
            job: Job::Sprayed,
            date: today(),
            product: None,
            amount: String::new(),
            unit: AmountUnit::Kg,
            note: String::new(),
        }
    }

    fn from_entry(e: &LogEntry) -> LogSeed {
        LogSeed {
            id: Some(e.id),
            plants: e.plants.clone(),
            job: e.job,
            date: e.date,
            product: e.product,
            amount: e.amount.map(|a| a.value.to_string()).unwrap_or_default(),
            unit: e.amount.map_or(AmountUnit::Kg, |a| a.unit),
            note: e.note.clone(),
        }
    }
}

/// Which Garden mode card is showing what.
#[derive(Clone, Copy)]
pub struct CareUi {
    pub products_open: RwSignal<bool>,
    pub form: RwSignal<Option<LogSeed>>,
}

pub fn provide_care_ui() {
    provide_context(CareUi {
        products_open: RwSignal::new(false),
        form: RwSignal::new(None),
    });
}

fn template(t: &str, pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .fold(t.to_string(), |acc, (k, v)| acc.replace(k, v))
}

fn unit_key(u: AmountUnit) -> &'static str {
    match u {
        AmountUnit::Kg => "kg",
        AmountUnit::G => "g",
        AmountUnit::Lb => "lb",
        AmountUnit::Pieces => "pieces",
        AmountUnit::L => "l",
        AmountUnit::Ml => "ml",
    }
}

fn product_kind_key(k: ProductKind) -> &'static str {
    match k {
        ProductKind::Fungicide => "fungicide",
        ProductKind::Insecticide => "insecticide",
        ProductKind::Herbicide => "herbicide",
        ProductKind::Fertiliser => "fertiliser",
        ProductKind::Other => "other",
    }
}

fn amount_text(value: f64, unit: AmountUnit) -> String {
    let v = format!("{value:.3}");
    let v = v.trim_end_matches('0').trim_end_matches('.');
    format!("{v} {}", amount_unit_label(&EN, unit))
}

/// Health as four buttons; `current` is None when several plants differ.
fn health_buttons(
    state: RwSignal<EditorState>,
    ids: Vec<ObjectId>,
    current: Option<Health>,
) -> impl IntoView {
    let buttons = HEALTHS
        .into_iter()
        .map(|h| {
            let ids = ids.clone();
            view! {
                <button class=format!("health-{}", health_label(&EN, h).to_lowercase())
                    aria-pressed=(current == Some(h)).to_string()
                    on:click=move |_| state.update(|s| s.set_health(&ids, h, now()))>
                    {health_label(&EN, h)}
                </button>
            }
        })
        .collect_view();
    view! {
        <div class="care-section">
            <h3>{EN.health_label}</h3>
            <div class="segmented health-buttons" role="group" aria-label=EN.health_label>{buttons}</div>
        </div>
    }
}

/// "Safe to harvest from …", "not again before …" and season counts for one plant.
fn safe_lines(s: &EditorState, id: ObjectId, day: Date) -> Vec<String> {
    let doc = s.doc();
    let mut lines = Vec::new();
    if let Some(d) = safe_to_harvest(doc, id) {
        lines.push(if d <= day {
            EN.safe_now.to_string()
        } else {
            template(EN.safe_from, &[("{date}", date_label(d))])
        });
    }
    for n in not_before(doc, id, day) {
        lines.push(template(
            EN.not_again,
            &[("{product}", n.name), ("{date}", date_label(n.date))],
        ));
    }
    for p in &doc.products {
        let Some(max) = p.max_per_season else {
            continue;
        };
        let count = season_count(doc, id, p.id, day.year());
        if count > 0 {
            lines.push(template(
                EN.season_count,
                &[
                    ("{product}", p.name.clone()),
                    ("{count}", count.to_string()),
                    ("{max}", max.to_string()),
                ],
            ));
        }
    }
    lines
}

fn history_view(
    state: RwSignal<EditorState>,
    ui: CareUi,
    s: &EditorState,
    id: ObjectId,
) -> AnyView {
    let doc = s.doc();
    let entries = history(doc, id);
    if entries.is_empty() {
        return view! { <p class="care-empty">{EN.history_empty}</p> }.into_any();
    }
    entries
        .into_iter()
        .map(|e| {
            let warnings: Vec<String> = entry_warnings(doc, e)
                .iter()
                .filter(|w| match w {
                    garden_editor::care::Warning::TooSoon { plant, .. }
                    | garden_editor::care::Warning::OverSeason { plant, .. }
                    | garden_editor::care::Warning::HarvestTooEarly { plant, .. } => *plant == id,
                })
                .map(|w| warning_text(doc, w))
                .collect();
            let mut what = job_label(&EN, e.job).to_string();
            if !e.product_name.is_empty() {
                what.push_str(&format!(" · {}", e.product_name));
            }
            if let Some(a) = e.amount {
                what.push_str(&format!(" · {}", amount_text(a.value, a.unit)));
            }
            let others = e.plants.len() - 1;
            let with = match others {
                0 => None,
                1 => Some(EN.with_one_other.to_string()),
                n => Some(template(EN.with_others, &[("{n}", n.to_string())])),
            };
            let with_title = (others > 0).then(|| {
                e.plants
                    .iter()
                    .filter(|p| **p != id)
                    .map(|p| plant_name(doc, *p))
                    .collect::<Vec<_>>()
                    .join(", ")
            });
            let note = (!e.note.is_empty()).then(|| view! { <p class="entry-note">{e.note.clone()}</p> });
            let warn = warnings
                .into_iter()
                .map(|w| view! { <p class="entry-warning" role="note">{w}</p> })
                .collect_view();
            let seed = LogSeed::from_entry(e);
            let entry_id = e.id;
            view! {
                <li class="entry">
                    <div class="entry-head">
                        <time datetime=e.date.to_string()>{date_label(e.date)}</time>
                        <span class="entry-what">{what}</span>
                    </div>
                    {with.map(|w| view! { <small class="entry-with" title=with_title>{w}</small> })}
                    {note}
                    {warn}
                    <div class="entry-actions">
                        <button on:click=move |_| ui.form.set(Some(seed.clone()))>{EN.edit}</button>
                        <button on:click=move |_| state.update(|s| s.remove_entry(entry_id, now()))>{EN.delete}</button>
                    </div>
                </li>
            }
        })
        .collect_view()
        .into_any()
}

fn single_view(state: RwSignal<EditorState>, ui: CareUi, id: ObjectId) -> AnyView {
    state.with_untracked(|s| {
        let doc = s.doc();
        let Some(o) = doc.object(id) else {
            return ().into_any();
        };
        let units = doc.units;
        let care = s.care_of(id);
        let size = match &o.kind {
            ObjectKind::Plant { spread, height, .. } => template(
                EN.plant_size,
                &[
                    ("{spread}", format_length(*spread, units)),
                    ("{height}", format_length(*height, units)),
                ],
            ),
            ObjectKind::Linear { shape, height, .. } => template(
                EN.hedge_size,
                &[
                    ("{length}", format_length(shape.length(), units)),
                    ("{height}", format_length(*height, units)),
                ],
            ),
            _ => String::new(),
        };
        let day = today();
        let lines = safe_lines(s, id, day)
            .into_iter()
            .map(|l| view! { <li>{l}</li> })
            .collect_view();
        let gone = care.health == Health::Gone;
        let notes = care.notes.clone();
        view! {
            <header class="care-head">
                <div>
                    <h2>{o.name.clone()}</h2>
                    <small>{format!("{} · {size}", kind_label(&EN, &o.kind))}</small>
                </div>
                <button class="icon-btn" aria-label=EN.close
                    on:click=move |_| state.update(|s| s.selection.clear())>{icon(Icon::Close)}</button>
            </header>
            {health_buttons(state, vec![id], Some(care.health))}
            <ul class="safe-lines">{lines}</ul>
            <label class="care-section form-field">
                <h3>{EN.notes_label}</h3>
                <textarea rows="3" placeholder=EN.notes_placeholder prop:value=notes
                    on:change=move |e| {
                        let v = event_target_value(&e);
                        state.update(|s| s.set_notes(id, &v, now()));
                    }></textarea>
            </label>
            {crate::photos::photo_strip(state, id)}
            <div class="care-section">
                <h3>{EN.history_label}</h3>
                <ul class="history">{history_view(state, ui, s, id)}</ul>
            </div>
            {if gone {
                view! { <p class="care-empty">{EN.gone_no_log}</p> }.into_any()
            } else {
                view! {
                    <button class="trace-primary" on:click=move |_| ui.form.set(Some(LogSeed::new(vec![id])))>
                        {EN.log_job}
                    </button>
                }.into_any()
            }}
        }
        .into_any()
    })
}

fn several_view(state: RwSignal<EditorState>, ui: CareUi, ids: Vec<ObjectId>) -> AnyView {
    state.with_untracked(|s| {
        let doc = s.doc();
        let names = ids
            .iter()
            .map(|id| plant_name(doc, *id))
            .collect::<Vec<_>>()
            .join(", ");
        let healths: Vec<Health> = ids.iter().map(|id| s.care_of(*id).health).collect();
        let current = healths
            .first()
            .copied()
            .filter(|h| healths.iter().all(|x| x == h));
        let day = today();
        let safe = group_safe_to_harvest(doc, &ids).map(|d| {
            if d <= day {
                EN.safe_now.to_string()
            } else {
                template(EN.safe_from, &[("{date}", date_label(d))])
            }
        });
        let all_gone = healths.iter().all(|h| *h == Health::Gone);
        let log_ids = ids.clone();
        view! {
            <header class="care-head">
                <div>
                    <h2>{template(EN.plants_selected, &[("{n}", ids.len().to_string())])}</h2>
                    <small>{names}</small>
                </div>
                <button class="icon-btn" aria-label=EN.close
                    on:click=move |_| state.update(|s| s.selection.clear())>{icon(Icon::Close)}</button>
            </header>
            {health_buttons(state, ids.clone(), current)}
            <ul class="safe-lines">{safe.map(|l| view! { <li>{l}</li> })}</ul>
            {(!all_gone).then(|| view! {
                <button class="trace-primary" on:click=move |_| ui.form.set(Some(LogSeed::new(log_ids.clone())))>
                    {EN.log_job}
                </button>
            })}
        }
        .into_any()
    })
}

/// The job form: new or editing an entry.
#[component]
fn LogForm(seed: LogSeed) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ui = expect_context::<CareUi>();
    let job = RwSignal::new(seed.job);
    let date = RwSignal::new(seed.date.to_string());
    let product = RwSignal::new(seed.product);
    let amount = RwSignal::new(seed.amount.clone());
    let unit = RwSignal::new(seed.unit);
    let note = RwSignal::new(seed.note.clone());
    let error = RwSignal::new(None::<&'static str>);
    let warnings = RwSignal::new(None::<Vec<String>>);
    let new_product = RwSignal::new(false);
    let plants = StoredValue::new(seed.plants.clone());
    let id = seed.id;
    let max_day = today().to_string();

    let draft = move || -> Result<LogDraft, &'static str> {
        let date = Date::parse(&date.get_untracked()).ok_or(EN.err_future_date)?;
        let amount = match parse_price(&amount.get_untracked()) {
            Ok(Some(v)) => Some(garden_core::model::Amount {
                value: v,
                unit: unit.get_untracked(),
            }),
            Ok(None) => None,
            Err(_) => return Err(EN.err_bad_amount),
        };
        let job = job.get_untracked();
        Ok(LogDraft {
            id,
            date,
            job,
            plants: plants.get_value(),
            product: matches!(job, Job::Sprayed | Job::Fed)
                .then(|| product.get_untracked())
                .flatten(),
            amount: matches!(job, Job::Sprayed | Job::Fed | Job::Harvested)
                .then_some(amount)
                .flatten(),
            note: note.get_untracked(),
        })
    };
    let save = move |anyway: bool| {
        let d = match draft() {
            Ok(d) => d,
            Err(m) => return error.set(Some(m)),
        };
        if !anyway {
            let checked = state.with_untracked(|s| {
                let entry = s.draft_entry(&d, now()).map_err(|e| e.message(&EN))?;
                Ok::<_, &'static str>(
                    entry_warnings(s.doc(), &entry)
                        .iter()
                        .map(|w| warning_text(s.doc(), w))
                        .collect::<Vec<_>>(),
                )
            });
            match checked {
                Err(m) => return error.set(Some(m)),
                Ok(w) if !w.is_empty() && d.date <= today() => {
                    error.set(None);
                    return warnings.set(Some(w));
                }
                Ok(_) => {}
            }
        }
        let mut result = Ok(());
        state.update(|s| {
            result = s.save_entry(d, today(), now()).map(|_| ());
        });
        match result {
            Ok(()) => ui.form.set(None),
            Err(e) => {
                warnings.set(None);
                error.set(Some(e.message(&EN)));
            }
        }
    };

    let job_buttons = JOBS
        .into_iter()
        .map(|j| {
            view! {
                <button type="button" aria-pressed=move || (job.get() == j).to_string()
                    on:click=move |_| job.set(j)>{job_label(&EN, j)}</button>
            }
        })
        .collect_view();
    let product_options = move || {
        state.with(|s| {
            s.doc()
                .products
                .iter()
                .map(|p| {
                    let pid = p.id;
                    view! {
                        <option value=pid.0.to_string() selected=move || product.get() == Some(pid)>
                            {p.name.clone()}
                        </option>
                    }
                })
                .collect_view()
        })
    };
    let unit_options = move || {
        UNITS
            .into_iter()
            .map(|u| {
                view! {
                    <option value=unit_key(u) selected=move || unit.get() == u>{amount_unit_label(&EN, u)}</option>
                }
            })
            .collect_view()
    };
    let uses_product = move || matches!(job.get(), Job::Sprayed | Job::Fed);
    let uses_amount = move || matches!(job.get(), Job::Sprayed | Job::Fed | Job::Harvested);
    let heading = if id.is_some() { EN.edit } else { EN.log_job };

    view! {
        <form class="plant-form log-form" novalidate on:submit=move |e| { e.prevent_default(); save(false); }
            on:keydown=move |e: leptos::ev::KeyboardEvent| if e.key() == "Escape" {
                e.prevent_default();
                ui.form.set(None);
            }>
            <h3>{heading}</h3>
            <div class="form-field">
                <span>{EN.job_label}</span>
                <div class="job-buttons" role="group" aria-label=EN.job_label>{job_buttons}</div>
            </div>
            <label class="form-field">
                <span>{EN.date_field}</span>
                <input type="date" max=max_day prop:value=move || date.get()
                    on:input=move |e| date.set(event_target_value(&e)) />
            </label>
            <Show when=uses_product>
                <label class="form-field">
                    <span>{EN.product_label}</span>
                    <select on:change=move |e| {
                        let v = event_target_value(&e);
                        if v == "new" {
                            new_product.set(true);
                            return;
                        }
                        let found = state.with_untracked(|s| {
                            s.doc().products.iter().find(|p| p.id.0.to_string() == v).map(|p| p.id)
                        });
                        product.set(found);
                    }>
                        <option value="" selected=move || product.get().is_none()>
                            {move || if job.get() == Job::Sprayed { EN.pick_product } else { EN.no_product }}
                        </option>
                        {product_options}
                        <option value="new">{EN.new_product}</option>
                    </select>
                </label>
            </Show>
            <Show when=move || new_product.get()>
                <ProductForm seed=None on_done=Callback::new(move |saved: Option<ProductId>| {
                    new_product.set(false);
                    if saved.is_some() {
                        product.set(saved);
                    }
                }) />
            </Show>
            <Show when=uses_amount>
                <div class="form-row">
                    <label class="form-field">
                        <span>{EN.amount_label}</span>
                        <input type="text" inputmode="decimal" prop:value=move || amount.get()
                            on:input=move |e| amount.set(event_target_value(&e)) />
                    </label>
                    <label class="form-field">
                        <span>"\u{a0}"</span>
                        <select aria-label=EN.amount_label on:change=move |e| {
                            let v = event_target_value(&e);
                            if let Some(u) = UNITS.into_iter().find(|u| unit_key(*u) == v) {
                                unit.set(u);
                            }
                        }>{unit_options}</select>
                    </label>
                </div>
            </Show>
            <label class="form-field">
                <span>{EN.note_label}</span>
                <textarea rows="2" prop:value=move || note.get()
                    on:input=move |e| note.set(event_target_value(&e))></textarea>
            </label>
            {move || error.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
            {move || warnings.get().map(|w| view! {
                <div class="log-warnings" role="alert">
                    <strong>{EN.warn_title}</strong>
                    <ul>{w.into_iter().map(|t| view! { <li>{t}</li> }).collect_view()}</ul>
                    <div class="form-actions">
                        <button type="button" class="trace-primary" on:click=move |_| save(true)>{EN.save_anyway}</button>
                        <button type="button" on:click=move |_| warnings.set(None)>{EN.back}</button>
                    </div>
                </div>
            })}
            <Show when=move || warnings.with(Option::is_none)>
                <div class="form-actions">
                    <button type="submit" class="trace-primary">{EN.save}</button>
                    <button type="button" on:click=move |_| ui.form.set(None)>{EN.cancel}</button>
                </div>
            </Show>
        </form>
    }
}

/// Add or edit a product. `on_done` gets the saved product's id, or None on cancel.
#[component]
fn ProductForm(seed: Option<Product>, on_done: Callback<Option<ProductId>>) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let days = |d: Option<u32>| d.map(|d| d.to_string()).unwrap_or_default();
    let id = seed.as_ref().map(|p| p.id);
    let name = RwSignal::new(seed.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let kind = RwSignal::new(seed.as_ref().map_or(ProductKind::Fungicide, |p| p.kind));
    let harvest = RwSignal::new(days(seed.as_ref().and_then(|p| p.harvest_days)));
    let reapply = RwSignal::new(days(seed.as_ref().and_then(|p| p.reapply_days)));
    let max = RwSignal::new(days(seed.as_ref().and_then(|p| p.max_per_season)));
    let notes = RwSignal::new(seed.as_ref().map(|p| p.notes.clone()).unwrap_or_default());
    let error = RwSignal::new(None::<&'static str>);
    let save = move || {
        let parsed = (
            parse_days(&harvest.get_untracked()),
            parse_days(&reapply.get_untracked()),
            parse_days(&max.get_untracked()),
        );
        let (Ok(harvest_days), Ok(reapply_days), Ok(max_per_season)) = parsed else {
            return error.set(Some(EN.err_bad_days));
        };
        let draft = ProductDraft {
            id,
            name: name.get_untracked(),
            kind: kind.get_untracked(),
            harvest_days,
            reapply_days,
            max_per_season,
            notes: notes.get_untracked(),
        };
        let mut result = None;
        state.update(|s| result = Some(s.save_product(draft, now())));
        match result {
            Some(Ok(id)) => on_done.run(Some(id)),
            Some(Err(e)) => error.set(Some(e.message(&EN))),
            None => {}
        }
    };
    let kind_options = KINDS
        .into_iter()
        .map(|k| {
            view! {
                <option value=product_kind_key(k) selected=move || kind.get() == k>{product_kind_label(&EN, k)}</option>
            }
        })
        .collect_view();
    let field = |label: &'static str, sig: RwSignal<String>| {
        view! {
            <label class="form-field">
                <span>{label}</span>
                <input type="text" inputmode="numeric" prop:value=move || sig.get()
                    on:input=move |e| sig.set(event_target_value(&e)) />
            </label>
        }
    };
    view! {
        // a div, not a form: it can sit inside the job form
        <div class="plant-form product-form" role="group" aria-label=EN.add_product
            on:keydown=move |e: leptos::ev::KeyboardEvent| {
                if e.key() == "Enter" && e.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlTextAreaElement>(t).ok()).is_none() {
                    e.prevent_default();
                    e.stop_propagation();
                    save();
                } else if e.key() == "Escape" {
                    e.prevent_default();
                    e.stop_propagation();
                    on_done.run(None);
                }
            }>
            <h3>{if id.is_some() { EN.edit } else { EN.add_product }}</h3>
            <p class="trace-note">{EN.products_note}</p>
            <label class="form-field">
                <span>{EN.product_name}</span>
                <input type="text" prop:value=move || name.get() on:input=move |e| name.set(event_target_value(&e)) />
            </label>
            <label class="form-field">
                <span>{EN.product_kind}</span>
                <select on:change=move |e| {
                    let v = event_target_value(&e);
                    if let Some(k) = KINDS.into_iter().find(|k| product_kind_key(*k) == v) {
                        kind.set(k);
                    }
                }>{kind_options}</select>
            </label>
            {field(EN.harvest_days, harvest)}
            <div class="form-row">
                {field(EN.reapply_days, reapply)}
                {field(EN.max_per_season, max)}
            </div>
            <label class="form-field">
                <span>{EN.product_notes}</span>
                <textarea rows="2" prop:value=move || notes.get() on:input=move |e| notes.set(event_target_value(&e))></textarea>
            </label>
            {move || error.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
            <div class="form-actions">
                <button type="button" class="trace-primary" on:click=move |_| save()>{EN.save}</button>
                <button type="button" on:click=move |_| on_done.run(None)>{EN.cancel}</button>
            </div>
        </div>
    }
}

/// The plant card: one plant, several plants, or the job form.
#[component]
pub fn PlantCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ui = expect_context::<CareUi>();
    let SelectSeveral(several) = expect_context::<SelectSeveral>();
    let garden = Memo::new(move |_| state.with(|s| s.mode() == Mode::Garden));
    let selection = Memo::new(move |_| state.with(|s| s.selection.clone()));
    // rebuild on document changes and selection changes, not on every pan or zoom
    let changes = Memo::new(move |_| state.with(|s| s.change_count()));
    // a new selection closes a job form that was for other plants
    Effect::new(move |_| {
        let sel = selection.get();
        if ui.form.with_untracked(|f| {
            f.as_ref()
                .is_some_and(|f| f.id.is_none() && f.plants != sel)
        }) {
            ui.form.set(None);
        }
    });
    let body = move || {
        // while the job form is open, only opening or closing it rebuilds the
        // card: a product added from the form, or an undo, must not wipe what
        // was typed
        if let Some(seed) = ui.form.get() {
            return view! { <LogForm seed=seed /> }.into_any();
        }
        changes.track();
        let ids = selection.get();
        match ids.as_slice() {
            [] => ().into_any(),
            [id] => single_view(state, ui, *id),
            _ => several_view(state, ui, ids),
        }
    };
    view! {
        <Show when=move || garden.get() && (!selection.with(Vec::is_empty) || ui.form.with(Option::is_some))>
            <section class="card plant-card" aria-label=EN.mode_garden>
                {body}
                <label class="select-several">
                    <input type="checkbox" prop:checked=move || several.get()
                        on:change=move |e| several.set(event_target_checked(&e)) />
                    {EN.select_several}
                </label>
            </section>
        </Show>
    }
}

/// The products library.
#[component]
pub fn ProductsCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ui = expect_context::<CareUi>();
    let garden = Memo::new(move |_| state.with(|s| s.mode() == Mode::Garden));
    let editing = RwSignal::new(None::<Option<Product>>);
    let products = Memo::new(move |_| state.with(|s| s.doc().products.clone()));
    let rules = |p: &Product| {
        let days = |d: Option<u32>| {
            d.map_or(EN.not_set.to_string(), |d| {
                template(EN.days_n, &[("{n}", d.to_string())])
            })
        };
        template(
            EN.product_rules,
            &[
                ("{harvest}", days(p.harvest_days)),
                ("{reapply}", days(p.reapply_days)),
                (
                    "{max}",
                    p.max_per_season
                        .map_or(EN.not_set.to_string(), |m| m.to_string()),
                ),
            ],
        )
    };
    let list = move || {
        let ps = products.get();
        if ps.is_empty() {
            return view! { <p class="care-empty">{EN.products_empty}</p> }.into_any();
        }
        ps.into_iter()
            .map(|p| {
                let id = p.id;
                let line = rules(&p);
                let edit = p.clone();
                view! {
                    <li class="product-row">
                        <div>
                            <strong>{p.name.clone()}</strong>
                            <small>{format!("{} · {line}", product_kind_label(&EN, p.kind))}</small>
                        </div>
                        <div class="entry-actions">
                            <button on:click=move |_| editing.set(Some(Some(edit.clone())))>{EN.edit}</button>
                            <button on:click=move |_| state.update(|s| s.remove_product(id, now()))>{EN.delete}</button>
                        </div>
                    </li>
                }
            })
            .collect_view()
            .into_any()
    };
    view! {
        <Show when=move || garden.get() && ui.products_open.get()>
            <section class="card products-card" aria-label=EN.products_title>
                <header class="trace-head">
                    <h2>{EN.products_title}</h2>
                    <button class="icon-btn" aria-label=EN.close
                        on:click=move |_| ui.products_open.set(false)>{icon(Icon::Close)}</button>
                </header>
                {move || match editing.get() {
                    Some(seed) => view! {
                        <ProductForm seed=seed on_done=Callback::new(move |_| editing.set(None)) />
                    }.into_any(),
                    None => view! {
                        <ul class="products">{list}</ul>
                        <button class="trace-primary" on:click=move |_| editing.set(Some(None))>{EN.add_product}</button>
                    }.into_any(),
                }}
            </section>
        </Show>
    }
}
