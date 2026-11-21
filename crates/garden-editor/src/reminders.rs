//! Reminders without a server: the due jobs of the next weeks, the count for
//! the app icon and banner, and a calendar (.ics) file for the phone's calendar.

use std::collections::{BTreeMap, BTreeSet};

use garden_core::date::Date;
use garden_core::model::{CareTask, Document, ObjectId, TaskId};
use serde::Serialize;

use crate::calendar::{covered, next_after, plant_due_on, target_text};
use crate::care::plant_name;
use crate::strings::{EN, job_label};

/// How far ahead reminders and the calendar file look.
pub const DAYS_AHEAD: i32 = 60;

/// One job due on one day, for the plants due that day.
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub task: TaskId,
    pub date: Date,
    pub plants: Vec<ObjectId>,
}

/// Due occurrences from `today` to `today + days`, one per job per date.
/// Overdue jobs are put on today; later ones are projected as if each were
/// done on its date.
pub fn occurrences(doc: &Document, today: Date, days: i32) -> Vec<Occurrence> {
    let end = today.add_days(days);
    let mut out: Vec<Occurrence> = Vec::new();
    for task in &doc.tasks {
        let mut dated: BTreeMap<Date, Vec<ObjectId>> = BTreeMap::new();
        for plant in covered(doc, task) {
            let mut date = plant_due_on(doc, task, plant, today).date.max(today);
            while date <= end {
                dated.entry(date).or_default().push(plant);
                let next = next_after(task, date);
                if next <= date {
                    break;
                }
                date = next;
            }
        }
        out.extend(dated.into_iter().map(|(date, plants)| Occurrence {
            task: task.id,
            date,
            plants,
        }));
    }
    // by date, then in the order the jobs were set up
    let order = |id: TaskId| doc.tasks.iter().position(|t| t.id == id);
    out.sort_by_key(|o| (o.date, order(o.task)));
    out
}

/// "Fed · Seaweed feed — all apples (3)".
pub fn occurrence_title(doc: &Document, task: &CareTask, plants: usize) -> String {
    let mut title = job_label(&EN, task.job).to_string();
    if !task.product_name.is_empty() {
        title.push_str(" · ");
        title.push_str(&task.product_name);
    }
    title.push_str(" — ");
    title.push_str(&target_text(doc, task));
    if plants > 1 {
        title.push_str(&format!(" ({plants})"));
    }
    title
}

/// One line of the due summary the service worker reads.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DueItem {
    pub design_id: String,
    pub design: String,
    pub task: String,
    pub date: String,
    pub title: String,
    pub plants: usize,
}

fn task_of(doc: &Document, id: TaskId) -> Option<&CareTask> {
    doc.tasks.iter().find(|t| t.id == id)
}

/// The design's occurrences over the next `DAYS_AHEAD` days, as summary items.
pub fn due_items(doc: &Document, today: Date) -> Vec<DueItem> {
    occurrences(doc, today, DAYS_AHEAD)
        .into_iter()
        .filter_map(|o| {
            let task = task_of(doc, o.task)?;
            Some(DueItem {
                design_id: doc.id.to_string(),
                design: doc.name.clone(),
                task: o.task.0.to_string(),
                date: o.date.to_string(),
                title: occurrence_title(doc, task, o.plants.len()),
                plants: o.plants.len(),
            })
        })
        .collect()
}

/// Jobs with any plant overdue or due today.
pub fn due_count(doc: &Document, today: Date) -> usize {
    doc.tasks
        .iter()
        .filter(|t| {
            covered(doc, t)
                .into_iter()
                .any(|p| plant_due_on(doc, t, p, today).date <= today)
        })
        .count()
}

/// The service worker's rule: distinct (design, job) with a date on or before today.
pub fn summary_count(items: &[DueItem], today: Date) -> usize {
    let today = today.to_string();
    items
        .iter()
        .filter(|i| i.date <= today)
        .map(|i| (&i.design_id, &i.task))
        .collect::<BTreeSet<_>>()
        .len()
}

/// Escape text for an iCalendar value.
fn ics_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// One content line, folded at 75 octets without splitting a character.
fn ics_line(out: &mut String, line: &str) {
    let mut used = 0;
    let mut limit = 75;
    for c in line.chars() {
        let n = c.len_utf8();
        if used + n > limit {
            out.push_str("\r\n ");
            used = 0;
            limit = 74;
        }
        out.push(c);
        used += n;
    }
    out.push_str("\r\n");
}

fn ics_date(d: Date) -> String {
    let (y, m, day) = d.ymd();
    format!("{y:04}{m:02}{day:02}")
}

fn ics_stamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400) as i32;
    let rest = secs.rem_euclid(86_400);
    let date = Date::parse("1970-01-01").expect("valid").add_days(days);
    format!(
        "{}T{:02}{:02}{:02}Z",
        ics_date(date),
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The due jobs as an RFC 5545 calendar file.
pub fn to_ics(doc: &Document, today: Date, now_utc_ms: i64) -> String {
    let mut out = String::new();
    for l in [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//Garden Planner//EN",
        "CALSCALE:GREGORIAN",
    ] {
        ics_line(&mut out, l);
    }
    let stamp = ics_stamp(now_utc_ms);
    for o in occurrences(doc, today, DAYS_AHEAD) {
        let Some(task) = task_of(doc, o.task) else {
            continue;
        };
        let title = ics_text(&occurrence_title(doc, task, o.plants.len()));
        let mut description = o
            .plants
            .iter()
            .map(|p| plant_name(doc, *p))
            .collect::<Vec<_>>()
            .join(", ");
        if !task.note.is_empty() {
            description.push_str("\n\n");
            description.push_str(&task.note);
        }
        for l in [
            "BEGIN:VEVENT".to_string(),
            format!("UID:{}-{}@garden-planner", task.id.0, ics_date(o.date)),
            format!("DTSTAMP:{stamp}"),
            format!("DTSTART;VALUE=DATE:{}", ics_date(o.date)),
            format!("DTEND;VALUE=DATE:{}", ics_date(o.date.add_days(1))),
            format!("SUMMARY:{title}"),
            format!("DESCRIPTION:{}", ics_text(&description)),
            "BEGIN:VALARM".to_string(),
            "ACTION:DISPLAY".to_string(),
            format!("DESCRIPTION:{title}"),
            "TRIGGER:PT8H".to_string(),
            "END:VALARM".to_string(),
            "END:VEVENT".to_string(),
        ] {
            ics_line(&mut out, &l);
        }
    }
    ics_line(&mut out, "END:VCALENDAR");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::doc_with;
    use garden_core::geometry::Point;
    use garden_core::model::{
        Health, Job, LogEntry, LogId, ObjectKind, PlantCare, PlantCategory, PlantSymbol, Repeat,
        Target,
    };

    fn day(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn today() -> Date {
        day("2026-06-10")
    }

    fn apple(x: f64) -> ObjectKind {
        ObjectKind::Plant {
            position: Point::new(x, 0.),
            spread: 3000.,
            height: 3000.,
            symbol: PlantSymbol::Round,
            category: PlantCategory::FruitTree,
            catalog_id: Some("apple".into()),
        }
    }

    fn doc() -> (Document, ObjectId, ObjectId) {
        let mut d = doc_with(vec![apple(0.), apple(5000.)]);
        d.objects[0].name = "Apple A".into();
        d.objects[1].name = "Apple B".into();
        let (a, b) = (d.objects[0].id, d.objects[1].id);
        (d, a, b)
    }

    fn task(job: Job, repeat: Repeat, months: Option<(u32, u32)>, first: &str) -> CareTask {
        CareTask {
            id: TaskId::new(),
            job,
            product: None,
            product_name: String::new(),
            note: String::new(),
            target: Target::Kind {
                catalog_id: "apple".into(),
                name: "Apple Tree".into(),
            },
            repeat,
            months,
            first_due: day(first),
            skips: Default::default(),
        }
    }

    fn log(d: &mut Document, job: Job, date: &str, plants: &[ObjectId]) {
        d.log.push(LogEntry {
            id: LogId::new(),
            date: day(date),
            job,
            plants: plants.to_vec(),
            plant_names: plants.iter().map(|_| "x".to_string()).collect(),
            product: None,
            product_name: String::new(),
            amount: None,
            note: String::new(),
            created_ms: 0,
        });
    }

    fn dates(occ: &[Occurrence]) -> Vec<String> {
        occ.iter().map(|o| o.date.to_string()).collect()
    }

    #[test]
    fn one_event_per_job_per_day() {
        let (mut d, a, b) = doc();
        d.tasks.push(task(
            Job::Fed,
            Repeat::Every { days: 28 },
            None,
            "2026-06-20",
        ));
        let occ = occurrences(&d, today(), 20);
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].date, day("2026-06-20"));
        assert_eq!(occ[0].plants, vec![a, b]);
        log(&mut d, Job::Fed, "2026-06-08", &[b]);
        let occ = occurrences(&d, today(), 30);
        assert_eq!(dates(&occ), vec!["2026-06-20", "2026-07-06"]);
        assert_eq!(occ[0].plants, vec![a]);
        assert_eq!(occ[1].plants, vec![b]);
    }

    #[test]
    fn overdue_goes_on_today() {
        let (mut d, _, _) = doc();
        d.tasks.push(task(
            Job::Watered,
            Repeat::Every { days: 7 },
            None,
            "2026-06-01",
        ));
        let occ = occurrences(&d, today(), DAYS_AHEAD);
        let got = dates(&occ);
        assert_eq!(got.first().map(String::as_str), Some("2026-06-10"));
        assert_eq!(got[1], "2026-06-17");
        assert_eq!(got.last().map(String::as_str), Some("2026-08-05"));
        assert_eq!(got.len(), 9);
        assert!(occ.iter().all(|o| o.date <= today().add_days(DAYS_AHEAD)));
    }

    #[test]
    fn months_and_seasons_project() {
        let (mut d, a, _) = doc();
        let winter = task(
            Job::Pruned,
            Repeat::Every { days: 28 },
            Some((11, 2)),
            "2026-11-01",
        );
        log(&mut d, Job::Pruned, "2026-11-22", &[a]);
        d.tasks.push(winter);
        let from = day("2026-12-20");
        let occ: Vec<Occurrence> = occurrences(&d, from, DAYS_AHEAD)
            .into_iter()
            .filter(|o| o.plants.contains(&a))
            .collect();
        assert_eq!(dates(&occ), vec!["2026-12-20", "2027-01-17", "2027-02-14"]);
        let (mut d, _, _) = doc();
        d.tasks.push(task(
            Job::Pruned,
            Repeat::Yearly,
            Some((2, 2)),
            "2026-01-01",
        ));
        // February 2026 was missed; the next season is February 2027
        let occ = occurrences(&d, from, DAYS_AHEAD);
        assert_eq!(dates(&occ), vec!["2027-02-01"]);
    }

    #[test]
    fn excluded_plants_never_show() {
        let (mut d, a, b) = doc();
        d.tasks.push(task(
            Job::Fed,
            Repeat::Every { days: 7 },
            None,
            "2026-06-10",
        ));
        d.care.insert(
            b,
            PlantCare {
                health: Health::Gone,
                ..PlantCare::default()
            },
        );
        let occ = occurrences(&d, today(), 14);
        assert!(occ.iter().all(|o| o.plants == vec![a]));
        d.layers[0].visible = false;
        assert!(occurrences(&d, today(), 14).is_empty());
    }

    #[test]
    fn counts_agree() {
        let (mut d, a, b) = doc();
        d.tasks.push(task(
            Job::Watered,
            Repeat::Every { days: 7 },
            None,
            "2026-06-01",
        ));
        d.tasks.push(task(
            Job::Fed,
            Repeat::Every { days: 28 },
            None,
            "2026-06-12",
        ));
        log(&mut d, Job::Fed, "2026-05-20", &[a]);
        let _ = b;
        for t in [today(), today().add_days(3)] {
            let items = due_items(&d, t);
            assert_eq!(due_count(&d, t), summary_count(&items, t), "{t}");
        }
        assert_eq!(due_count(&d, today()), 1);
        assert_eq!(due_count(&d, today().add_days(3)), 2);
        assert!(due_items(&d, today()).iter().all(|i| i.design == "g"));
    }

    fn unfold(s: &str) -> String {
        s.replace("\r\n ", "")
    }

    #[test]
    fn ics_is_well_formed() {
        let empty = to_ics(&doc().0, today(), 0);
        assert!(empty.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(empty.ends_with("END:VCALENDAR\r\n"));
        assert!(!empty.contains("BEGIN:VEVENT"));

        let (mut d, _, _) = doc();
        let mut t = task(Job::Fed, Repeat::Yearly, Some((12, 12)), "2026-12-31");
        t.note = "a;b,c\\d\nnext".into();
        t.product_name = format!("Engrais {}", "é".repeat(80));
        let id = t.id;
        d.tasks.push(t);
        let ics = to_ics(&d, day("2026-12-01"), 1_795_000_000_000);
        for line in ics.split_inclusive("\r\n") {
            assert!(line.ends_with("\r\n"), "{line:?}");
            assert!(line.len() <= 77, "{} octets: {line:?}", line.len() - 2);
        }
        assert!(!ics.replace("\r\n", "").contains('\n'), "bare LF");
        let flat = unfold(&ics);
        assert!(flat.contains(&format!("Engrais {}", "é".repeat(80))));
        assert!(flat.contains(r"a\;b\,c\\d\nnext"));
        assert!(flat.contains("DTSTART;VALUE=DATE:20261231\r\n"));
        assert!(flat.contains("DTEND;VALUE=DATE:20270101\r\n"));
        assert!(flat.contains(&format!("UID:{}-20261231@garden-planner\r\n", id.0)));
        assert_eq!(
            flat.matches("BEGIN:VALARM").count(),
            flat.matches("BEGIN:VEVENT").count()
        );
        assert!(flat.contains("TRIGGER:PT8H\r\n"));
        assert!(flat.contains("DTSTAMP:2026"));
    }
}
