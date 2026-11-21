//! The care calendar: when each repeating job is next due for each plant, and
//! how the This week card groups them. Pure; "today" is always passed in.

use garden_core::date::Date;
use garden_core::model::{
    CareTask, Document, Health, Job, ObjectId, ObjectKind, PlantCategory, ProductId, Repeat,
    Target, TaskId,
};

use crate::care::{date_label, is_gardenable, season_count};
use crate::strings::{EN, category_label};

fn day(y: i32, m: u32, d: u32) -> Date {
    Date::from_ymd(y, m, d).unwrap_or_else(|| Date::parse("1970-01-01").expect("valid"))
}

/// The last day of month `m` in year `y`.
fn month_end(y: i32, m: u32) -> Date {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    day(ny, nm, 1).add_days(-1)
}

/// The first day of the month after `d`.
fn next_month_start(d: Date) -> Date {
    let (y, m, _) = d.ymd();
    if m == 12 {
        day(y + 1, 1, 1)
    } else {
        day(y, m + 1, 1)
    }
}

/// The plants a job covers now: existing plants and hedges that aren't gone
/// and aren't on a hidden layer.
pub fn covered(doc: &Document, task: &CareTask) -> Vec<ObjectId> {
    let ok = |id: ObjectId| {
        doc.object(id).is_some_and(|o| {
            is_gardenable(o)
                && doc.layer(o.layer).is_some_and(|l| l.visible)
                && doc.care.get(&id).map(|c| c.health) != Some(Health::Gone)
        })
    };
    let mut out: Vec<ObjectId> = match &task.target {
        Target::Plants { ids } => ids.clone(),
        Target::Kind { catalog_id, .. } => doc
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Plant { catalog_id: Some(c), .. } if c == catalog_id))
            .map(|o| o.id)
            .collect(),
        Target::Category { category } => doc
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Plant { category: c, .. } if c == category))
            .map(|o| o.id)
            .collect(),
    };
    let mut seen = std::collections::HashSet::new();
    out.retain(|id| ok(*id) && seen.insert(*id));
    out
}

/// Every date the job happened for `plant`: logged entries and the last skip.
fn done_dates(doc: &Document, task: &CareTask, plant: ObjectId) -> Vec<Date> {
    let mut dates: Vec<Date> = doc
        .log
        .iter()
        .filter(|e| {
            e.job == task.job
                && e.plants.contains(&plant)
                && (task.product.is_none() || e.product == task.product)
        })
        .map(|e| e.date)
        .collect();
    dates.extend(task.skips.get(&plant).copied());
    dates
}

/// When the job last happened for `plant`: logged, or skipped.
pub fn last_date(doc: &Document, task: &CareTask, plant: ObjectId) -> Option<Date> {
    done_dates(doc, task, plant).into_iter().max()
}

/// Whether `d` falls in the months `(from, to)`; `from > to` wraps past the year end.
pub fn in_months(months: (u32, u32), d: Date) -> bool {
    let (from, to) = months;
    let m = d.ymd().1;
    if from <= to {
        (from..=to).contains(&m)
    } else {
        m >= from || m <= to
    }
}

/// `d`, or the first day of the next allowed month when `d` is outside the months.
fn into_months(months: Option<(u32, u32)>, d: Date) -> Date {
    let Some(months) = months else { return d };
    if in_months(months, d) {
        return d;
    }
    let mut m = next_month_start(d);
    for _ in 0..12 {
        if in_months(months, m) {
            return m;
        }
        m = next_month_start(m);
    }
    d
}

/// Why a plant's due date was moved later by the product label.
#[derive(Clone, Debug, PartialEq)]
pub enum Hold {
    NotBefore {
        product: String,
        date: Date,
    },
    SeasonFull {
        product: String,
        count: u32,
        max: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlantDue {
    pub plant: ObjectId,
    pub date: Date,
    pub hold: Option<Hold>,
}

/// The once-a-year due date: the first season ending on or after first_due
/// with nothing done in it, from its start (or first_due, if later).
fn yearly_due(task: &CareTask, (from, to): (u32, u32), done: &[Date]) -> Date {
    let first = task.first_due;
    for y in (first.year() - 1)..=(first.year() + 200) {
        let start = day(y, from, 1);
        let end = if from <= to {
            month_end(y, to)
        } else {
            month_end(y + 1, to)
        };
        if end < first || done.iter().any(|d| *d >= start && *d <= end) {
            continue;
        }
        return start.max(first);
    }
    first
}

/// The last application of `product` on `plant`, from the log.
fn last_application(doc: &Document, product: ProductId, plant: ObjectId) -> Option<Date> {
    doc.log
        .iter()
        .filter(|e| {
            matches!(e.job, Job::Sprayed | Job::Fed)
                && e.product == Some(product)
                && e.plants.contains(&plant)
        })
        .map(|e| e.date)
        .max()
}

/// The due date from the repeat and the months alone, before label limits.
fn base_date(task: &CareTask, done: &[Date]) -> Date {
    match (task.repeat, task.months) {
        (Repeat::Yearly, Some(months)) => yearly_due(task, months, done),
        (repeat, months) => {
            let days = match repeat {
                Repeat::Every { days } => days,
                Repeat::Yearly => 365,
            };
            let base = done
                .iter()
                .max()
                .map_or(task.first_due, |d| d.add_days(days as i32));
            into_months(months, base)
        }
    }
}

/// When the job would next be due if it were done on `done_on`, from the
/// repeat and the months alone (no label limits). Used to project ahead.
pub fn next_after(task: &CareTask, done_on: Date) -> Date {
    match (task.repeat, task.months) {
        // the first season after the one this was done in
        (Repeat::Yearly, Some(months)) => {
            let from = CareTask {
                first_due: done_on,
                ..task.clone()
            };
            yearly_due(&from, months, &[done_on])
        }
        _ => base_date(task, &[done_on]),
    }
}

/// Move `date` later for the product label: the gap between applications,
/// then the yearly maximum. The months still apply after each move.
fn apply_label(doc: &Document, task: &CareTask, plant: ObjectId, mut date: Date) -> PlantDue {
    let mut hold = None;
    let product = task
        .product
        .filter(|_| matches!(task.job, Job::Sprayed | Job::Fed))
        .and_then(|id| doc.products.iter().find(|p| p.id == id));
    if let Some(p) = product {
        if let Some(gap) = p.reapply_days
            && let Some(last) = last_application(doc, p.id, plant)
        {
            let allowed = last.add_days(gap as i32);
            if allowed > date {
                date = into_months(task.months, allowed);
                hold = Some(Hold::NotBefore {
                    product: p.name.clone(),
                    date: allowed,
                });
            }
        }
        if let Some(max) = p.max_per_season {
            let count = season_count(doc, plant, p.id, date.year());
            if count >= max {
                date = into_months(task.months, day(date.year() + 1, 1, 1));
                hold = Some(Hold::SeasonFull {
                    product: p.name.clone(),
                    count,
                    max,
                });
            }
        }
    }
    PlantDue { plant, date, hold }
}

/// When `task` is next due for `plant`, by the rules alone (no "today").
pub fn plant_due(doc: &Document, task: &CareTask, plant: ObjectId) -> PlantDue {
    let done = done_dates(doc, task, plant);
    apply_label(doc, task, plant, base_date(task, &done))
}

/// Whether some month from the one after `from` up to `to`'s month is outside `months`.
fn left_months(months: (u32, u32), from: Date, to: Date) -> bool {
    let mut m = next_month_start(from);
    while m <= to {
        if !in_months(months, m) {
            return true;
        }
        m = next_month_start(m);
    }
    false
}

/// When `task` is due for `plant` as seen on `today`: a missed job whose
/// season (its months) has ended waits for the next season instead.
pub fn plant_due_on(doc: &Document, task: &CareTask, plant: ObjectId, today: Date) -> PlantDue {
    let done = done_dates(doc, task, plant);
    let mut base = base_date(task, &done);
    if let Some(months) = task.months
        && base < today
        && left_months(months, base, today)
    {
        base = match task.repeat {
            Repeat::Yearly => {
                let from_today = CareTask {
                    first_due: today,
                    ..task.clone()
                };
                yearly_due(&from_today, months, &done)
            }
            Repeat::Every { .. } => into_months(Some(months), today),
        };
    }
    apply_label(doc, task, plant, base)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Overdue,
    ThisWeek,
    ComingUp,
    Later,
}

/// The last day a section reaches; None for Later.
fn section_end(section: Section, today: Date) -> Option<Date> {
    match section {
        Section::Overdue => Some(today.add_days(-1)),
        Section::ThisWeek => Some(today.add_days(6)),
        Section::ComingUp => Some(today.add_days(36)),
        Section::Later => None,
    }
}

/// Which section of the This week card a due date belongs in.
pub fn section_of(due: Date, today: Date) -> Section {
    [Section::Overdue, Section::ThisWeek, Section::ComingUp]
        .into_iter()
        .find(|s| section_end(*s, today).is_some_and(|end| due <= end))
        .unwrap_or(Section::Later)
}

/// One line of the This week card.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskLine {
    pub task: TaskId,
    pub section: Section,
    /// Earliest due date among the covered plants; None when there are none.
    pub due: Option<Date>,
    pub covered: usize,
    /// Covered plants due by the end of this line's section.
    pub due_plants: Vec<ObjectId>,
    /// Plants whose date the label moved, with the reason.
    pub holds: Vec<(ObjectId, Hold)>,
}

fn line(doc: &Document, task: &CareTask, today: Date) -> TaskLine {
    let dues: Vec<PlantDue> = covered(doc, task)
        .into_iter()
        .map(|p| plant_due_on(doc, task, p, today))
        .collect();
    let due = dues.iter().map(|d| d.date).min();
    let section = due.map_or(Section::Later, |d| section_of(d, today));
    let end = section_end(section, today);
    TaskLine {
        task: task.id,
        section,
        due,
        covered: dues.len(),
        due_plants: dues
            .iter()
            .filter(|d| end.is_none_or(|e| d.date <= e))
            .map(|d| d.plant)
            .collect(),
        holds: dues
            .iter()
            .filter_map(|d| d.hold.clone().map(|h| (d.plant, h)))
            .collect(),
    }
}

/// Every repeating job as a line, sorted by section then due date.
pub fn week_lines(doc: &Document, today: Date) -> Vec<TaskLine> {
    let mut lines: Vec<TaskLine> = doc.tasks.iter().map(|t| line(doc, t, today)).collect();
    lines.sort_by_key(|l| (l.section, l.due.is_none(), l.due));
    lines
}

/// Plants with something overdue or due today.
pub fn plan_dots(doc: &Document, today: Date) -> Vec<ObjectId> {
    let mut out: Vec<ObjectId> = Vec::new();
    for task in &doc.tasks {
        for p in covered(doc, task) {
            if !out.contains(&p) && plant_due_on(doc, task, p, today).date <= today {
                out.push(p);
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComingUp {
    pub task: TaskId,
    pub job: Job,
    pub date: Date,
}

/// The next (at most 3) jobs due for `plant`, soonest first.
pub fn coming_up(doc: &Document, plant: ObjectId, today: Date) -> Vec<ComingUp> {
    let mut out: Vec<ComingUp> = doc
        .tasks
        .iter()
        .filter(|t| covered(doc, t).contains(&plant))
        .map(|t| ComingUp {
            task: t.id,
            job: t.job,
            date: plant_due_on(doc, t, plant, today).date,
        })
        .collect();
    out.sort_by_key(|c| c.date);
    out.truncate(3);
    out
}

fn month_span((from, to): (u32, u32)) -> String {
    let name = |m: u32| EN.months[(m.clamp(1, 12) - 1) as usize];
    if from == to {
        name(from).to_string()
    } else {
        format!("{}–{}", name(from), name(to))
    }
}

/// A catalog kind's name (built-in or the person's own plants).
pub fn kind_name(doc: &Document, catalog_id: &str) -> Option<String> {
    crate::catalog::catalog_with(&doc.custom_plants)
        .into_iter()
        .find(|e| e.id == catalog_id)
        .map(|e| e.name)
}

/// "every 4 weeks, Apr–Sep", "every 10 days", "once a year, Nov–Feb".
pub fn repeat_text(task: &CareTask) -> String {
    let base = match task.repeat {
        Repeat::Every { days: 1 } => EN.repeat_every_day.to_string(),
        Repeat::Every { days: 7 } => EN.repeat_every_week.to_string(),
        Repeat::Every { days } if days % 7 == 0 => EN
            .repeat_every_weeks
            .replace("{n}", &(days / 7).to_string()),
        Repeat::Every { days } => EN.repeat_every_days.replace("{n}", &days.to_string()),
        Repeat::Yearly => EN.repeat_yearly.to_string(),
    };
    match task.months {
        Some(m) => format!("{base}, {}", month_span(m)),
        None => base,
    }
}

/// "Copper: not again before 4 Dec 2026", "Copper: 3 of 3 this year".
pub fn hold_text(h: &Hold) -> String {
    match h {
        Hold::NotBefore { product, date } => EN
            .not_again
            .replace("{product}", product)
            .replace("{date}", &date_label(*date)),
        Hold::SeasonFull {
            product,
            count,
            max,
        } => EN
            .season_count
            .replace("{product}", product)
            .replace("{count}", &count.to_string())
            .replace("{max}", &max.to_string()),
    }
}

/// What a job covers, in words: a plant's name, "2 plants", "every Apple Tree".
pub fn target_text(doc: &Document, task: &CareTask) -> String {
    match &task.target {
        Target::Plants { ids } if ids.len() == 1 => crate::care::plant_name(doc, ids[0]),
        Target::Plants { ids } => EN.plants_n.replace("{n}", &ids.len().to_string()),
        Target::Kind { name, .. } => EN.every_target.replace("{name}", name),
        Target::Category {
            category: PlantCategory::Other,
        } => EN.all_other_plants.to_string(),
        Target::Category { category } => EN
            .all_target
            .replace("{name}", &category_label(&EN, *category).to_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::doc_with;
    use garden_core::command::Command;
    use garden_core::geometry::Point;
    use garden_core::model::{
        Health, LogEntry, LogId, ObjectKind, PlantCare, PlantCategory, PlantSymbol, Product,
        ProductId, ProductKind, Repeat, Target,
    };

    fn day(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn plant(x: f64, catalog: &str, category: PlantCategory) -> ObjectKind {
        ObjectKind::Plant {
            position: Point::new(x, 0.),
            spread: 3000.,
            height: 3000.,
            symbol: PlantSymbol::Round,
            category,
            catalog_id: Some(catalog.into()),
        }
    }

    struct Fx {
        doc: Document,
        a: ObjectId,
        b: ObjectId,
        l: ObjectId,
        copper: ProductId,
    }

    fn today() -> Date {
        day("2026-06-10")
    }

    /// Apples A and B (catalog "apple", FruitTree), lavender L (Perennial);
    /// Copper: 21 days before harvest, 14 between, 3 a year.
    fn fixture() -> Fx {
        let mut doc = doc_with(vec![
            plant(0., "apple", PlantCategory::FruitTree),
            plant(5000., "apple", PlantCategory::FruitTree),
            plant(10000., "lavender", PlantCategory::Perennial),
        ]);
        doc.objects[0].name = "Apple A".into();
        doc.objects[1].name = "Apple B".into();
        doc.objects[2].name = "Lavender".into();
        let copper = ProductId::new();
        doc.products.push(Product {
            id: copper,
            name: "Copper".into(),
            kind: ProductKind::Fungicide,
            harvest_days: Some(21),
            reapply_days: Some(14),
            max_per_season: Some(3),
            notes: String::new(),
        });
        let (a, b, l) = (doc.objects[0].id, doc.objects[1].id, doc.objects[2].id);
        Fx {
            doc,
            a,
            b,
            l,
            copper,
        }
    }

    fn task(
        target: Target,
        job: Job,
        repeat: Repeat,
        months: Option<(u32, u32)>,
        first: &str,
    ) -> CareTask {
        CareTask {
            id: TaskId::new(),
            job,
            product: None,
            product_name: String::new(),
            note: String::new(),
            target,
            repeat,
            months,
            first_due: day(first),
            skips: Default::default(),
        }
    }

    fn apples() -> Target {
        Target::Kind {
            catalog_id: "apple".into(),
            name: "Apple Tree".into(),
        }
    }

    fn log(fx: &mut Fx, job: Job, date: &str, plants: &[ObjectId], product: Option<ProductId>) {
        fx.doc.log.push(LogEntry {
            id: LogId::new(),
            date: day(date),
            job,
            plants: plants.to_vec(),
            plant_names: plants.iter().map(|_| "x".to_string()).collect(),
            product,
            product_name: if product.is_some() {
                "Copper".into()
            } else {
                String::new()
            },
            amount: None,
            note: String::new(),
            created_ms: fx.doc.log.len() as i64,
        });
    }

    fn every(days: u32) -> Repeat {
        Repeat::Every { days }
    }

    #[test]
    fn covers_kind_category_and_list() {
        let mut fx = fixture();
        let kind = task(apples(), Job::Fed, every(28), None, "2026-06-01");
        assert_eq!(covered(&fx.doc, &kind), vec![fx.a, fx.b]);
        let cat = task(
            Target::Category {
                category: PlantCategory::FruitTree,
            },
            Job::Fed,
            every(28),
            None,
            "2026-06-01",
        );
        assert_eq!(covered(&fx.doc, &cat), vec![fx.a, fx.b]);
        let list = task(
            Target::Plants {
                ids: vec![fx.l, ObjectId::new()],
            },
            Job::Fed,
            every(28),
            None,
            "2026-06-01",
        );
        assert_eq!(covered(&fx.doc, &list), vec![fx.l]);
        // a third apple added later joins
        let layer = fx.doc.layers[0].id;
        fx.doc
            .apply(
                Command::Insert {
                    object: garden_core::model::Object::new(
                        "Apple C",
                        plant(15000., "apple", PlantCategory::FruitTree),
                        layer,
                    ),
                    index: 3,
                },
                1,
            )
            .unwrap();
        assert_eq!(covered(&fx.doc, &kind).len(), 3);
        // gone, or on a hidden layer, drops out
        fx.doc.care.insert(
            fx.b,
            PlantCare {
                health: Health::Gone,
                ..PlantCare::default()
            },
        );
        assert_eq!(covered(&fx.doc, &kind).len(), 2);
        fx.doc.layers[0].visible = false;
        assert!(covered(&fx.doc, &kind).is_empty());
    }

    #[test]
    fn every_counts_from_last_done() {
        let mut fx = fixture();
        let mut t = task(apples(), Job::Fed, every(28), None, "2026-06-01");
        assert_eq!(plant_due(&fx.doc, &t, fx.a).date, day("2026-06-01"));
        let a = fx.a;
        log(&mut fx, Job::Fed, "2026-06-05", &[a], None);
        log(&mut fx, Job::Watered, "2026-06-09", &[a], None);
        assert_eq!(last_date(&fx.doc, &t, fx.a), Some(day("2026-06-05")));
        assert_eq!(plant_due(&fx.doc, &t, fx.a).date, day("2026-07-03"));
        assert_eq!(section_of(day("2026-07-03"), today()), Section::ComingUp);
        assert_eq!(section_of(day("2026-06-01"), today()), Section::Overdue);
        assert_eq!(section_of(day("2026-06-16"), today()), Section::ThisWeek);
        assert_eq!(section_of(day("2026-06-17"), today()), Section::ComingUp);
        assert_eq!(section_of(day("2026-07-16"), today()), Section::ComingUp);
        assert_eq!(section_of(day("2026-07-17"), today()), Section::Later);
        t.skips.insert(fx.a, day("2026-06-20"));
        assert_eq!(plant_due(&fx.doc, &t, fx.a).date, day("2026-07-18"));
        // a task naming a product only counts entries with that product
        let mut sp = task(apples(), Job::Fed, every(28), None, "2026-06-01");
        sp.product = Some(fx.copper);
        assert_eq!(last_date(&fx.doc, &sp, fx.a), None);
    }

    #[test]
    fn months_move_and_wrap() {
        let mut fx = fixture();
        let a = fx.a;
        let summer = task(apples(), Job::Fed, every(28), Some((4, 9)), "2026-04-01");
        log(&mut fx, Job::Fed, "2026-09-20", &[a], None);
        assert_eq!(plant_due(&fx.doc, &summer, a).date, day("2027-04-01"));
        let winter = task(
            apples(),
            Job::Pruned,
            every(28),
            Some((11, 2)),
            "2026-01-01",
        );
        log(&mut fx, Job::Pruned, "2026-02-20", &[a], None);
        assert_eq!(plant_due(&fx.doc, &winter, a).date, day("2026-11-01"));
        assert!(in_months((11, 2), day("2027-01-15")));
        assert!(in_months((11, 2), day("2026-11-01")));
        assert!(!in_months((11, 2), day("2026-03-01")));
        assert!(in_months((4, 9), day("2026-09-30")));
        assert!(!in_months((4, 9), day("2026-10-01")));
        assert!(in_months((5, 5), day("2026-05-31")));
    }

    #[test]
    fn yearly_seasons() {
        let mut fx = fixture();
        let a = fx.a;
        let feb = task(
            apples(),
            Job::Pruned,
            Repeat::Yearly,
            Some((2, 2)),
            "2026-01-01",
        );
        assert_eq!(plant_due(&fx.doc, &feb, a).date, day("2026-02-01"));
        log(&mut fx, Job::Pruned, "2026-02-14", &[a], None);
        assert_eq!(plant_due(&fx.doc, &feb, a).date, day("2027-02-01"));
        let mut fx = fixture();
        let a = fx.a;
        let winter = task(
            apples(),
            Job::Sprayed,
            Repeat::Yearly,
            Some((11, 2)),
            "2026-10-01",
        );
        log(&mut fx, Job::Sprayed, "2027-01-20", &[a], None);
        assert_eq!(plant_due(&fx.doc, &winter, a).date, day("2027-11-01"));
        // created mid-season: due from first_due, not overdue since April
        let summer = task(
            apples(),
            Job::Fed,
            Repeat::Yearly,
            Some((4, 9)),
            "2026-06-15",
        );
        assert_eq!(plant_due(&fx.doc, &summer, a).date, day("2026-06-15"));
        // without months: every 365 days
        let plain = task(apples(), Job::Sprayed, Repeat::Yearly, None, "2026-01-01");
        assert_eq!(plant_due(&fx.doc, &plain, a).date, day("2028-01-20"));
    }

    #[test]
    fn label_holds_back() {
        let mut fx = fixture();
        let a = fx.a;
        let copper = fx.copper;
        let mut t = task(apples(), Job::Sprayed, every(7), None, "2026-01-01");
        t.product = Some(copper);
        t.product_name = "Copper".into();
        log(&mut fx, Job::Sprayed, "2026-06-05", &[a], Some(copper));
        let due = plant_due(&fx.doc, &t, a);
        assert_eq!(due.date, day("2026-06-19"));
        assert_eq!(
            due.hold,
            Some(Hold::NotBefore {
                product: "Copper".into(),
                date: day("2026-06-19")
            })
        );
        // a full year moves it to 1 January, then the months apply
        log(&mut fx, Job::Sprayed, "2026-02-01", &[a], Some(copper));
        log(&mut fx, Job::Sprayed, "2026-04-01", &[a], Some(copper));
        let due = plant_due(&fx.doc, &t, a);
        assert_eq!(due.date, day("2027-01-01"));
        assert_eq!(
            due.hold,
            Some(Hold::SeasonFull {
                product: "Copper".into(),
                count: 3,
                max: 3
            })
        );
        t.months = Some((4, 9));
        assert_eq!(plant_due(&fx.doc, &t, a).date, day("2027-04-01"));
        // a deleted product holds nothing back
        fx.doc
            .apply(Command::RemoveProduct { id: copper }, 1)
            .unwrap();
        t.months = None;
        let due = plant_due(&fx.doc, &t, a);
        assert_eq!(due.date, day("2026-06-12"));
        assert_eq!(due.hold, None);
    }

    #[test]
    fn lines_sections_and_due_plants() {
        let mut fx = fixture();
        let (a, b) = (fx.a, fx.b);
        let t = task(apples(), Job::Fed, every(28), None, "2026-01-01");
        log(&mut fx, Job::Fed, "2026-05-01", &[a], None);
        log(&mut fx, Job::Fed, "2026-06-08", &[b], None);
        fx.doc.tasks.push(t.clone());
        let gone = task(
            Target::Plants {
                ids: vec![ObjectId::new()],
            },
            Job::Pruned,
            every(7),
            None,
            "2026-01-01",
        );
        fx.doc.tasks.push(gone.clone());
        let lines = week_lines(&fx.doc, today());
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].task, t.id);
        assert_eq!(lines[0].section, Section::Overdue);
        assert_eq!(lines[0].due, Some(day("2026-05-29")));
        assert_eq!(lines[0].covered, 2);
        assert_eq!(lines[0].due_plants, vec![a]);
        assert_eq!(lines[1].task, gone.id);
        assert_eq!(lines[1].section, Section::Later);
        assert_eq!(lines[1].due, None);
        assert_eq!(lines[1].covered, 0);
        assert_eq!(plan_dots(&fx.doc, today()), vec![a]);
    }

    #[test]
    fn coming_up_and_texts() {
        let mut fx = fixture();
        let a = fx.a;
        for (job, days) in [
            (Job::Fed, 28),
            (Job::Watered, 7),
            (Job::Pruned, 90),
            (Job::Other, 3),
        ] {
            let mut t = task(apples(), job, every(days), None, "2026-06-10");
            t.first_due = today().add_days(days as i32);
            fx.doc.tasks.push(t);
        }
        let next = coming_up(&fx.doc, a, today());
        let jobs: Vec<Job> = next.iter().map(|c| c.job).collect();
        assert_eq!(jobs, vec![Job::Other, Job::Watered, Job::Fed]);
        let t = |r, m| task(apples(), Job::Fed, r, m, "2026-01-01");
        assert_eq!(repeat_text(&t(every(28), None)), "every 4 weeks");
        assert_eq!(repeat_text(&t(every(7), None)), "every week");
        assert_eq!(repeat_text(&t(every(1), None)), "every day");
        assert_eq!(repeat_text(&t(every(10), None)), "every 10 days");
        assert_eq!(
            repeat_text(&t(every(28), Some((4, 9)))),
            "every 4 weeks, Apr–Sep"
        );
        assert_eq!(
            repeat_text(&t(Repeat::Yearly, Some((11, 2)))),
            "once a year, Nov–Feb"
        );
        assert_eq!(
            repeat_text(&t(Repeat::Yearly, Some((2, 2)))),
            "once a year, Feb"
        );
        assert_eq!(
            hold_text(&Hold::NotBefore {
                product: "Copper".into(),
                date: day("2026-12-04")
            }),
            "Copper: not again before 4 Dec 2026"
        );
        assert_eq!(
            hold_text(&Hold::SeasonFull {
                product: "Copper".into(),
                count: 3,
                max: 3
            }),
            "Copper: 3 of 3 this year"
        );
        assert_eq!(target_text(&fx.doc, &t(every(7), None)), "every Apple Tree");
        let cat = task(
            Target::Category {
                category: PlantCategory::FruitTree,
            },
            Job::Fed,
            every(7),
            None,
            "2026-01-01",
        );
        assert_eq!(target_text(&fx.doc, &cat), "all fruit trees");
        let one = task(
            Target::Plants { ids: vec![fx.l] },
            Job::Fed,
            every(7),
            None,
            "2026-01-01",
        );
        assert_eq!(target_text(&fx.doc, &one), "Lavender");
        let two = task(
            Target::Plants {
                ids: vec![fx.a, fx.l],
            },
            Job::Fed,
            every(7),
            None,
            "2026-01-01",
        );
        assert_eq!(target_text(&fx.doc, &two), "2 plants");
    }

    #[test]
    fn label_respects_months() {
        let mut fx = fixture();
        let (a, copper) = (fx.a, fx.copper);
        let mut t = task(apples(), Job::Sprayed, every(7), Some((4, 9)), "2026-04-01");
        t.product = Some(copper);
        log(&mut fx, Job::Sprayed, "2026-09-20", &[a], Some(copper));
        let due = plant_due(&fx.doc, &t, a);
        assert_eq!(due.date, day("2027-04-01"));
        assert_eq!(
            due.hold,
            Some(Hold::NotBefore {
                product: "Copper".into(),
                date: day("2026-10-04")
            })
        );
    }

    #[test]
    fn missed_jobs_move_on() {
        let mut fx = fixture();
        let a = fx.a;
        // Apr–Sep feed missed in September waits for April once September is over
        let summer = task(apples(), Job::Fed, every(28), Some((4, 9)), "2026-04-01");
        log(&mut fx, Job::Fed, "2026-08-20", &[a], None);
        assert_eq!(
            plant_due_on(&fx.doc, &summer, a, day("2026-09-20")).date,
            day("2026-09-17")
        );
        assert_eq!(
            plant_due_on(&fx.doc, &summer, a, day("2026-10-05")).date,
            day("2027-04-01")
        );
        // a February pruning missed in February waits for next February
        let feb = task(
            apples(),
            Job::Pruned,
            Repeat::Yearly,
            Some((2, 2)),
            "2026-01-01",
        );
        assert_eq!(
            plant_due_on(&fx.doc, &feb, a, day("2026-02-20")).date,
            day("2026-02-01")
        );
        assert_eq!(
            plant_due_on(&fx.doc, &feb, a, day("2026-03-10")).date,
            day("2027-02-01")
        );
        // without months a missed job stays overdue
        let plain = task(apples(), Job::Watered, every(28), None, "2026-06-01");
        assert_eq!(
            plant_due_on(&fx.doc, &plain, a, today()).date,
            day("2026-06-01")
        );
        // the card uses it: the summer feed isn't overdue in October
        fx.doc.tasks.push(summer.clone());
        let lines = week_lines(&fx.doc, day("2026-10-05"));
        assert_eq!(lines[0].section, Section::Later);
        assert!(plan_dots(&fx.doc, day("2026-10-05")).is_empty());
    }

    #[test]
    fn kind_names_come_from_the_catalog() {
        let fx = fixture();
        let apple = crate::catalog::builtin_plants()
            .into_iter()
            .find(|p| p.id == "apple")
            .unwrap();
        assert_eq!(kind_name(&fx.doc, "apple"), Some(apple.name));
        assert_eq!(kind_name(&fx.doc, "no-such-plant"), None);
    }
}
