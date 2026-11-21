//! The care calendar in Garden mode: the This week card, the repeating-job
//! form, and the plant card's "Coming up" line.

use leptos::prelude::*;
use loma_core::date::Date;
use loma_core::model::{
    CareTask, Job, ObjectId, ObjectKind, PlantCategory, ProductId, Repeat, Target, TaskId,
};
use loma_editor::calendar::{
    Section, TaskLine, coming_up, hold_text, kind_name, repeat_text, target_text, week_lines,
};
use loma_editor::care::date_label;
use loma_editor::garden::TaskDraft;
use loma_editor::state::{EditorState, Mode};
use loma_editor::strings::{EN, job_label};

use crate::app::now;
use crate::care::{CareUi, JOBS, LogSeed};
use crate::garden::today;
use crate::icons::{Icon, icon};

/// What the repeating-job form starts with.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskSeed {
    id: Option<TaskId>,
    job: Job,
    product: Option<ProductId>,
    note: String,
    /// What the job can cover, as (words, target); `chosen` indexes it.
    options: Vec<(String, Target)>,
    chosen: usize,
    yearly: bool,
    every: String,
    weeks: bool,
    months: (Option<u32>, Option<u32>),
    first_due: Date,
}

fn label_of(doc: &loma_core::model::Document, target: &Target) -> String {
    let probe = CareTask {
        id: TaskId::new(),
        job: Job::Other,
        product: None,
        product_name: String::new(),
        note: String::new(),
        target: target.clone(),
        repeat: Repeat::Yearly,
        months: None,
        first_due: today(),
        skips: Default::default(),
    };
    target_text(doc, &probe)
}

impl TaskSeed {
    fn blank(options: Vec<(String, Target)>) -> TaskSeed {
        TaskSeed {
            id: None,
            job: Job::Fed,
            product: None,
            note: String::new(),
            options,
            chosen: 0,
            yearly: false,
            every: "4".into(),
            weeks: true,
            months: (None, None),
            first_due: today(),
        }
    }

    /// From a plant card: this plant, all of its kind, all of its category.
    pub fn for_plant(s: &EditorState, id: ObjectId) -> TaskSeed {
        let doc = s.doc();
        let mut options = Vec::new();
        if let Some(o) = doc.object(id) {
            match &o.kind {
                ObjectKind::Plant {
                    catalog_id,
                    category,
                    ..
                } => {
                    options.push((EN.this_plant.to_string(), Target::Plants { ids: vec![id] }));
                    if let Some(c) = catalog_id {
                        let t = Target::Kind {
                            catalog_id: c.clone(),
                            name: kind_name(doc, c).unwrap_or_else(|| o.name.clone()),
                        };
                        options.push((label_of(doc, &t), t));
                    }
                    let t = Target::Category {
                        category: *category,
                    };
                    options.push((label_of(doc, &t), t));
                }
                _ => options.push((EN.this_hedge.to_string(), Target::Plants { ids: vec![id] })),
            }
        }
        TaskSeed::blank(options)
    }

    /// From the This week card: the selection, then each category and kind in the design.
    pub fn for_week(s: &EditorState) -> TaskSeed {
        TaskSeed::blank(design_options(s))
    }

    fn from_task(s: &EditorState, t: &CareTask) -> TaskSeed {
        let doc = s.doc();
        let mut options = vec![(label_of(doc, &t.target), t.target.clone())];
        options.extend(
            design_options(s)
                .into_iter()
                .filter(|(_, target)| *target != t.target),
        );
        let (yearly, every, weeks) = match t.repeat {
            Repeat::Yearly => (true, "4".to_string(), true),
            Repeat::Every { days } if days % 7 == 0 => (false, (days / 7).to_string(), true),
            Repeat::Every { days } => (false, days.to_string(), false),
        };
        TaskSeed {
            id: Some(t.id),
            job: t.job,
            product: t.product,
            note: t.note.clone(),
            options,
            chosen: 0,
            yearly,
            every,
            weeks,
            months: t.months.map_or((None, None), |(a, b)| (Some(a), Some(b))),
            first_due: t.first_due,
        }
    }
}

fn design_options(s: &EditorState) -> Vec<(String, Target)> {
    let doc = s.doc();
    let mut out: Vec<(String, Target)> = Vec::new();
    if !s.selection.is_empty() {
        out.push((
            EN.selected_plants
                .replace("{n}", &s.selection.len().to_string()),
            Target::Plants {
                ids: s.selection.clone(),
            },
        ));
    }
    let mut categories: Vec<PlantCategory> = Vec::new();
    let mut kinds: Vec<(String, String)> = Vec::new();
    for o in &doc.objects {
        if let ObjectKind::Plant {
            category,
            catalog_id,
            ..
        } = &o.kind
        {
            if !categories.contains(category) {
                categories.push(*category);
            }
            if let Some(c) = catalog_id
                && !kinds.iter().any(|(id, _)| id == c)
            {
                kinds.push((
                    c.clone(),
                    kind_name(doc, c).unwrap_or_else(|| o.name.clone()),
                ));
            }
        }
    }
    for c in categories {
        let t = Target::Category { category: c };
        out.push((label_of(doc, &t), t));
    }
    for (catalog_id, name) in kinds {
        let t = Target::Kind { catalog_id, name };
        out.push((label_of(doc, &t), t));
    }
    out
}

fn section_label(s: Section) -> &'static str {
    match s {
        Section::Overdue => EN.sec_overdue,
        Section::ThisWeek => EN.sec_this_week,
        Section::ComingUp => EN.sec_coming_up,
        Section::Later => EN.sec_later,
    }
}

/// "3 days overdue", "today", "in 4 days" (within a week), else the date.
fn when(due: Date, day: Date) -> String {
    let d = due.days_since(day);
    match d {
        d if d < 0 => EN.overdue_days.replace("{n}", &(-d).to_string()),
        0 => EN.today_word.to_string(),
        1..=6 => EN.in_days.replace("{n}", &d.to_string()),
        _ => EN.due_on.replace("{date}", &date_label(due)),
    }
}

fn narrow() -> bool {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|w| w.as_f64())
        .is_some_and(|w| w < 768.0)
}

fn line_view(state: RwSignal<EditorState>, ui: CareUi, l: TaskLine) -> AnyView {
    state.with_untracked(|s| {
        let doc = s.doc();
        let Some(task) = doc.tasks.iter().find(|t| t.id == l.task).cloned() else {
            return ().into_any();
        };
        let day = today();
        let mut title = job_label(&EN, task.job).to_string();
        if !task.product_name.is_empty() {
            title.push_str(&format!(" · {}", task.product_name));
        }
        let mut who = target_text(doc, &task);
        if l.covered == 0 {
            who = EN.no_plants_left.to_string();
        } else if l.covered > 1 && l.section != Section::Later {
            who.push_str(&format!(
                " ({})",
                EN.n_of_m_due
                    .replace("{n}", &l.due_plants.len().to_string())
                    .replace("{m}", &l.covered.to_string())
            ));
        }
        let mut sub = repeat_text(&task);
        if let Some(due) = l.due {
            sub.push_str(&format!(" · {}", when(due, day)));
        }
        let holds = l
            .holds
            .iter()
            .map(|(_, h)| hold_text(h))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|t| view! { <p class="task-hold">{t}</p> })
            .collect_view();
        let edit = {
            let task = task.clone();
            move |_| {
                let seed = state.with_untracked(|s| TaskSeed::from_task(s, &task));
                ui.task_form.set(Some(seed));
            }
        };
        let actions = (l.section != Section::Later && !l.due_plants.is_empty()).then(|| {
            let plants = l.due_plants.clone();
            let skip_plants = l.due_plants.clone();
            let (job, product, note, id) = (task.job, task.product, task.note.clone(), task.id);
            view! {
                <div class="task-actions">
                    <button class="task-done" on:click=move |_| {
                        let plants = plants.clone();
                        state.update(|s| s.selection = plants.clone());
                        ui.form.set(Some(LogSeed::prefilled(plants, job, product, note.clone())));
                        if narrow() {
                            ui.week_open.set(false);
                        }
                    }>{EN.done}</button>
                    <button on:click=move |_| state.update(|s| s.skip_task(id, &skip_plants, today(), now()))>
                        {EN.skip}
                    </button>
                </div>
            }
        });
        view! {
            <li class="task-line">
                <button class="task-text" on:click=edit>
                    <strong>{title}</strong>
                    <span>{who}</span>
                    <small>{sub}</small>
                </button>
                {holds}
                {actions}
            </li>
        }
        .into_any()
    })
}

/// The plant card's "Coming up" line.
pub fn coming_up_view(s: &EditorState, id: ObjectId) -> impl IntoView + use<> {
    let day = today();
    let items: Vec<String> = coming_up(s.doc(), id, day)
        .into_iter()
        .map(|c| format!("{} · {}", job_label(&EN, c.job), when(c.date, day)))
        .collect();
    (!items.is_empty()).then(|| {
        view! {
            <div class="care-section">
                <h3>{EN.coming_up_label}</h3>
                <ul class="coming-up">{items.into_iter().map(|t| view! { <li>{t}</li> }).collect_view()}</ul>
            </div>
        }
    })
}

/// The This week card.
#[component]
pub fn WeekCard() -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ui = expect_context::<CareUi>();
    let garden = Memo::new(move |_| state.with(|s| s.mode() == Mode::Garden));
    let changes = Memo::new(move |_| state.with(|s| s.change_count()));
    let show_later = RwSignal::new(false);
    let body = move || {
        if let Some(seed) = ui.task_form.get() {
            return view! { <TaskForm seed=seed /> }.into_any();
        }
        // read, not just track: a memo that was never read doesn't follow the state
        let _ = changes.get();
        let lines = state.with_untracked(|s| week_lines(s.doc(), today()));
        let any_due = lines.iter().any(|l| l.section != Section::Later);
        let sections = [Section::Overdue, Section::ThisWeek, Section::ComingUp]
            .into_iter()
            .filter_map(|sec| {
                let ls: Vec<TaskLine> = lines.iter().filter(|l| l.section == sec).cloned().collect();
                (!ls.is_empty()).then(|| {
                    view! {
                        <h3 class=format!("week-heading sec-{}", section_label(sec).to_lowercase().replace(' ', "-"))>
                            {section_label(sec)}
                        </h3>
                        <ul class="task-lines">{ls.into_iter().map(|l| line_view(state, ui, l)).collect_view()}</ul>
                    }
                })
            })
            .collect_view();
        let later: Vec<TaskLine> = lines
            .into_iter()
            .filter(|l| l.section == Section::Later)
            .collect();
        let later_view = (!later.is_empty()).then(|| {
            let n = later.len();
            view! {
                <button class="later-toggle" aria-expanded=move || show_later.get().to_string()
                    on:click=move |_| show_later.update(|v| *v = !*v)>
                    {format!("{} ({n})", EN.sec_later)}
                </button>
                <Show when=move || show_later.get()>
                    <ul class="task-lines">{later.clone().into_iter().map(|l| line_view(state, ui, l)).collect_view()}</ul>
                </Show>
            }
        });
        view! {
            {(!any_due).then(|| view! { <p class="care-empty">{EN.week_empty}</p> })}
            {sections}
            {later_view}
            <button class="trace-primary" on:click=move |_| {
                let seed = state.with_untracked(TaskSeed::for_week);
                ui.task_form.set(Some(seed));
            }>{EN.new_task}</button>
            <crate::reminders::ReminderControls />
        }
        .into_any()
    };
    view! {
        <Show when=move || garden.get() && ui.week_open.get()>
            <section class="card week-card" aria-label=EN.week_title>
                <header class="trace-head">
                    <h2>{EN.week_title}</h2>
                    <button class="icon-btn" aria-label=EN.close on:click=move |_| {
                        ui.week_open.set(false);
                        ui.task_form.set(None);
                    }>{icon(Icon::Close)}</button>
                </header>
                {body}
            </section>
        </Show>
    }
}

/// Set up or edit a repeating job.
#[component]
fn TaskForm(seed: TaskSeed) -> impl IntoView {
    let state = expect_context::<RwSignal<EditorState>>();
    let ui = expect_context::<CareUi>();
    let id = seed.id;
    let options = StoredValue::new(seed.options.clone());
    let job = RwSignal::new(seed.job);
    let product = RwSignal::new(seed.product);
    let note = RwSignal::new(seed.note.clone());
    let chosen = RwSignal::new(seed.chosen);
    let yearly = RwSignal::new(seed.yearly);
    let every = RwSignal::new(seed.every.clone());
    let weeks = RwSignal::new(seed.weeks);
    let from = RwSignal::new(seed.months.0);
    let to = RwSignal::new(seed.months.1);
    let first_due = RwSignal::new(seed.first_due.to_string());
    let error = RwSignal::new(None::<&'static str>);

    let save = move || {
        let repeat = if yearly.get_untracked() {
            Repeat::Yearly
        } else {
            match every.get_untracked().trim().parse::<u32>() {
                Ok(n) if (1..=365).contains(&n) => Repeat::Every {
                    days: if weeks.get_untracked() { n * 7 } else { n },
                },
                _ => return error.set(Some(EN.err_task_repeat)),
            }
        };
        let months = match (from.get_untracked(), to.get_untracked()) {
            (Some(a), Some(b)) => Some((a, b)),
            (None, None) => None,
            _ => return error.set(Some(EN.err_task_months)),
        };
        let Some(target) =
            options.with_value(|o| o.get(chosen.get_untracked()).map(|(_, t)| t.clone()))
        else {
            return error.set(Some(EN.err_task_no_plants));
        };
        let job_now = job.get_untracked();
        let draft = TaskDraft {
            id,
            job: job_now,
            product: matches!(job_now, Job::Sprayed | Job::Fed)
                .then(|| product.get_untracked())
                .flatten(),
            note: note.get_untracked(),
            target,
            repeat,
            months,
            first_due: Date::parse(&first_due.get_untracked()).unwrap_or_else(today),
        };
        let mut result = None;
        state.update(|s| result = Some(s.save_task(draft, now())));
        match result {
            Some(Ok(_)) => ui.task_form.set(None),
            Some(Err(e)) => error.set(Some(e.message(&EN))),
            None => {}
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
    let for_options = options.with_value(|o| {
        o.iter()
            .enumerate()
            .map(|(i, (label, _))| {
                view! { <option value=i.to_string() selected=move || chosen.get() == i>{label.clone()}</option> }
            })
            .collect_view()
    });
    let month_options = move |sig: RwSignal<Option<u32>>| {
        let any =
            view! { <option value="" selected=move || sig.get().is_none()>{EN.any_month}</option> };
        let months = (1..=12u32)
            .map(|m| {
                view! {
                    <option value=m.to_string() selected=move || sig.get() == Some(m)>
                        {EN.months[(m - 1) as usize]}
                    </option>
                }
            })
            .collect_view();
        view! { {any}{months} }
    };
    let month_change = move |sig: RwSignal<Option<u32>>| {
        move |e: leptos::ev::Event| sig.set(event_target_value(&e).parse().ok())
    };

    view! {
        <form class="plant-form task-form" novalidate on:submit=move |e| { e.prevent_default(); save(); }
            on:keydown=move |e: leptos::ev::KeyboardEvent| if e.key() == "Escape" {
                e.prevent_default();
                ui.task_form.set(None);
            }>
            <h3>{if id.is_some() { EN.edit } else { EN.new_task }}</h3>
            <div class="form-field">
                <span>{EN.job_label}</span>
                <div class="job-buttons" role="group" aria-label=EN.job_label>{job_buttons}</div>
            </div>
            <Show when=move || matches!(job.get(), Job::Sprayed | Job::Fed)>
                <label class="form-field">
                    <span>{EN.product_label}</span>
                    <select on:change=move |e| {
                        let v = event_target_value(&e);
                        let found = state.with_untracked(|s| {
                            s.doc().products.iter().find(|p| p.id.0.to_string() == v).map(|p| p.id)
                        });
                        product.set(found);
                    }>
                        <option value="" selected=move || product.get().is_none()>
                            {move || if job.get() == Job::Sprayed { EN.pick_product } else { EN.no_product }}
                        </option>
                        {product_options}
                    </select>
                </label>
            </Show>
            <label class="form-field">
                <span>{EN.task_for}</span>
                <select on:change=move |e| {
                    if let Ok(i) = event_target_value(&e).parse() {
                        chosen.set(i);
                    }
                }>{for_options}</select>
            </label>
            <div class="form-field">
                <span>{EN.task_repeats}</span>
                <div class="job-buttons" role="group" aria-label=EN.task_repeats>
                    <button type="button" aria-pressed=move || (!yearly.get()).to_string()
                        on:click=move |_| yearly.set(false)>{EN.task_every}</button>
                    <button type="button" aria-pressed=move || yearly.get().to_string()
                        on:click=move |_| yearly.set(true)>{EN.task_yearly}</button>
                </div>
            </div>
            <Show when=move || !yearly.get()>
                <div class="form-row">
                    <label class="form-field">
                        <span>{EN.task_every}</span>
                        <input type="text" inputmode="numeric" prop:value=move || every.get()
                            on:input=move |e| every.set(event_target_value(&e)) />
                    </label>
                    <label class="form-field">
                        <span>"\u{a0}"</span>
                        <select aria-label=EN.task_every on:change=move |e| weeks.set(event_target_value(&e) == "weeks")>
                            <option value="days" selected=move || !weeks.get()>{EN.unit_days}</option>
                            <option value="weeks" selected=move || weeks.get()>{EN.unit_weeks}</option>
                        </select>
                    </label>
                </div>
            </Show>
            <div class="form-row">
                <label class="form-field">
                    <span>{format!("{} · {}", EN.task_months, EN.month_from)}</span>
                    <select on:change=month_change(from)>{month_options(from)}</select>
                </label>
                <label class="form-field">
                    <span>{EN.month_to}</span>
                    <select on:change=month_change(to)>{month_options(to)}</select>
                </label>
            </div>
            <Show when=move || yearly.get() && from.get().is_none() && to.get().is_none()>
                <p class="trace-note">{EN.yearly_no_months}</p>
            </Show>
            <label class="form-field">
                <span>{EN.first_due}</span>
                <input type="date" prop:value=move || first_due.get()
                    on:input=move |e| first_due.set(event_target_value(&e)) />
            </label>
            <label class="form-field">
                <span>{EN.note_label}</span>
                <textarea rows="2" prop:value=move || note.get()
                    on:input=move |e| note.set(event_target_value(&e))></textarea>
            </label>
            {move || error.get().map(|m| view! { <p class="file-message" role="alert">{m}</p> })}
            <div class="form-actions">
                <button type="submit" class="trace-primary">{EN.save}</button>
                <button type="button" on:click=move |_| ui.task_form.set(None)>{EN.cancel}</button>
                {id.map(|id| view! {
                    <button type="button" class="task-delete" on:click=move |_| {
                        state.update(|s| s.remove_task(id, now()));
                        ui.task_form.set(None);
                    }>{EN.delete}</button>
                })}
            </div>
        </form>
    }
}
