//! Looking after the garden: which objects have plant cards, safe dates and
//! label warnings. Pure; "today" is always passed in.

use loma_core::date::Date;
use loma_core::model::{
    Document, Health, Job, LinearKind, LogEntry, Object, ObjectId, ObjectKind, Product, ProductId,
};

use crate::strings::EN;

/// Plants and hedges have plant cards; nothing else does.
pub fn is_gardenable(o: &Object) -> bool {
    matches!(
        o.kind,
        ObjectKind::Plant { .. }
            | ObjectKind::Linear {
                kind: LinearKind::Hedge,
                ..
            }
    )
}

/// One use of a product on a plant: a Sprayed or Fed entry whose product is
/// still in the library.
struct Application<'a> {
    date: Date,
    product: &'a Product,
}

fn applications<'a>(
    doc: &'a Document,
    log: impl IntoIterator<Item = &'a LogEntry>,
    plant: ObjectId,
) -> impl Iterator<Item = Application<'a>> {
    log.into_iter().filter_map(move |e| {
        if !matches!(e.job, Job::Sprayed | Job::Fed) || !e.plants.contains(&plant) {
            return None;
        }
        let product = doc.products.iter().find(|p| Some(p.id) == e.product)?;
        Some(Application {
            date: e.date,
            product,
        })
    })
}

fn safe_from<'a>(apps: impl Iterator<Item = Application<'a>>) -> Option<Date> {
    apps.filter_map(|a| Some(a.date.add_days(a.product.harvest_days? as i32)))
        .max()
}

/// When `plant` is next safe to harvest: the latest application date plus that
/// product's days before harvest. None when no application has that interval.
pub fn safe_to_harvest(doc: &Document, plant: ObjectId) -> Option<Date> {
    safe_from(applications(doc, &doc.log, plant))
}

/// Like [`safe_to_harvest`], counting only applications dated on or before `on`.
pub fn safe_to_harvest_on(doc: &Document, plant: ObjectId, on: Date) -> Option<Date> {
    safe_from(applications(doc, &doc.log, plant).filter(|a| a.date <= on))
}

/// A product that may not be applied to a plant again before `date`.
#[derive(Clone, Debug, PartialEq)]
pub struct NotBefore {
    pub product: ProductId,
    pub name: String,
    pub date: Date,
}

/// For each product: its last application on `plant` plus the days between
/// applications, when that is still after `today`. Sorted by date.
pub fn not_before(doc: &Document, plant: ObjectId, today: Date) -> Vec<NotBefore> {
    let mut out: Vec<NotBefore> = Vec::new();
    for a in applications(doc, &doc.log, plant) {
        let Some(days) = a.product.reapply_days else {
            continue;
        };
        let date = a.date.add_days(days as i32);
        match out.iter_mut().find(|n| n.product == a.product.id) {
            Some(n) => n.date = n.date.max(date),
            None => out.push(NotBefore {
                product: a.product.id,
                name: a.product.name.clone(),
                date,
            }),
        }
    }
    out.retain(|n| n.date > today);
    out.sort_by_key(|n| n.date);
    out
}

fn count_in_year<'a>(
    apps: impl Iterator<Item = Application<'a>>,
    product: ProductId,
    year: i32,
) -> u32 {
    apps.filter(|a| a.product.id == product && a.date.year() == year)
        .count() as u32
}

/// Applications of `product` on `plant` in calendar `year`.
pub fn season_count(doc: &Document, plant: ObjectId, product: ProductId, year: i32) -> u32 {
    count_in_year(applications(doc, &doc.log, plant), product, year)
}

/// A label rule an entry breaks, for one plant.
#[derive(Clone, Debug, PartialEq)]
pub enum Warning {
    /// Another application of the same product within `days` of this one.
    TooSoon {
        plant: ObjectId,
        product: String,
        other: Date,
        days: u32,
    },
    /// More applications in the entry's calendar year than the label allows.
    OverSeason {
        plant: ObjectId,
        product: String,
        year: i32,
        count: u32,
        max: u32,
    },
    /// A harvest before the plant was safe to harvest.
    HarvestTooEarly { plant: ObjectId, safe: Date },
}

/// The rules `entry` breaks, checked against the whole log as if `entry`
/// replaced any saved entry with the same id. Works for drafts and saved entries.
pub fn entry_warnings(doc: &Document, entry: &LogEntry) -> Vec<Warning> {
    let others: Vec<&LogEntry> = doc.log.iter().filter(|e| e.id != entry.id).collect();
    let mut out = Vec::new();
    for &plant in &entry.plants {
        match entry.job {
            Job::Sprayed | Job::Fed => {
                let Some(product) = doc.products.iter().find(|p| Some(p.id) == entry.product)
                else {
                    continue;
                };
                let same = || {
                    applications(doc, others.iter().copied(), plant)
                        .filter(|a| a.product.id == product.id)
                };
                if let Some(days) = product.reapply_days
                    && let Some(other) = same()
                        .filter(|a| a.date.days_since(entry.date).unsigned_abs() < days)
                        .min_by_key(|a| a.date.days_since(entry.date).unsigned_abs())
                {
                    out.push(Warning::TooSoon {
                        plant,
                        product: product.name.clone(),
                        other: other.date,
                        days,
                    });
                }
                if let Some(max) = product.max_per_season {
                    let year = entry.date.year();
                    let count = count_in_year(same(), product.id, year) + 1;
                    if count > max {
                        out.push(Warning::OverSeason {
                            plant,
                            product: product.name.clone(),
                            year,
                            count,
                            max,
                        });
                    }
                }
            }
            Job::Harvested => {
                let safe = safe_from(
                    applications(doc, others.iter().copied(), plant)
                        .filter(|a| a.date <= entry.date),
                );
                if let Some(safe) = safe
                    && entry.date < safe
                {
                    out.push(Warning::HarvestTooEarly { plant, safe });
                }
            }
            _ => {}
        }
    }
    out
}

/// "3 Jun 2024".
pub fn date_label(d: Date) -> String {
    let (y, m, day) = d.ymd();
    format!("{day} {} {y}", EN.months[(m - 1) as usize])
}

/// A plant's name: its current one, or the name saved in the log followed by
/// "(removed from plan)" when it is no longer on the plan.
pub fn plant_name(doc: &Document, plant: ObjectId) -> String {
    if let Some(o) = doc.object(plant) {
        return o.name.clone();
    }
    let saved = doc
        .log
        .iter()
        .rev()
        .find_map(|e| {
            let i = e.plants.iter().position(|p| *p == plant)?;
            e.plant_names.get(i).cloned()
        })
        .unwrap_or_default();
    format!("{saved} {}", EN.removed_from_plan)
        .trim()
        .to_string()
}

/// The warning as a sentence for the person.
pub fn warning_text(doc: &Document, w: &Warning) -> String {
    match w {
        Warning::TooSoon {
            plant,
            product,
            other,
            days,
        } => EN
            .warn_too_soon
            .replace("{product}", product)
            .replace("{plant}", &plant_name(doc, *plant))
            .replace("{date}", &date_label(*other))
            .replace("{days}", &days.to_string()),
        Warning::OverSeason {
            plant,
            product,
            year,
            count,
            max,
        } => EN
            .warn_over_season
            .replace("{product}", product)
            .replace("{plant}", &plant_name(doc, *plant))
            .replace("{count}", &count.to_string())
            .replace("{year}", &year.to_string())
            .replace("{max}", &max.to_string()),
        Warning::HarvestTooEarly { plant, safe } => EN
            .warn_harvest_early
            .replace("{plant}", &plant_name(doc, *plant))
            .replace("{date}", &date_label(*safe)),
    }
}

/// Log entries covering `plant`, newest first (by date, then by when they were logged).
pub fn history(doc: &Document, plant: ObjectId) -> Vec<&LogEntry> {
    let mut out: Vec<&LogEntry> = doc
        .log
        .iter()
        .filter(|e| e.plants.contains(&plant))
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse((e.date, e.created_ms)));
    out
}

fn health_of(doc: &Document, plant: ObjectId) -> Health {
    doc.care.get(&plant).map(|c| c.health).unwrap_or_default()
}

/// How a plant or hedge is marked on the plan in Garden mode.
#[derive(Clone, Debug, PartialEq)]
pub struct GardenMark {
    pub id: ObjectId,
    pub health: Health,
    /// Still inside a harvest interval today.
    pub not_safe: bool,
}

/// A mark for every plant and hedge.
pub fn garden_marks(doc: &Document, today: Date) -> Vec<GardenMark> {
    doc.objects
        .iter()
        .filter(|o| is_gardenable(o))
        .map(|o| GardenMark {
            id: o.id,
            health: health_of(doc, o.id),
            not_safe: safe_to_harvest(doc, o.id).is_some_and(|d| d > today),
        })
        .collect()
}

/// The latest safe-to-harvest date among `plants`, leaving out gone ones.
pub fn group_safe_to_harvest(doc: &Document, plants: &[ObjectId]) -> Option<Date> {
    plants
        .iter()
        .filter(|p| health_of(doc, **p) != Health::Gone)
        .filter_map(|p| safe_to_harvest(doc, *p))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{doc_with, plant_at};
    use loma_core::command::Command;
    use loma_core::model::{Amount, AmountUnit, Job, LogId, PlantCare, Product, ProductKind};

    fn day(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    struct Fx {
        doc: Document,
        a: ObjectId,
        b: ObjectId,
        copper: ProductId,
    }

    /// Two apples A and B, and Copper: 21 days before harvest, 14 between, 3 a year.
    fn fixture() -> Fx {
        let mut doc = doc_with(vec![plant_at(0., 0., 3000.), plant_at(5000., 0., 3000.)]);
        doc.objects[0].name = "Apple A".into();
        doc.objects[1].name = "Apple B".into();
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
        let (a, b) = (doc.objects[0].id, doc.objects[1].id);
        Fx { doc, a, b, copper }
    }

    fn entry(fx: &Fx, job: Job, date: &str, plants: &[ObjectId], created: i64) -> LogEntry {
        let sprayed = matches!(job, Job::Sprayed | Job::Fed);
        LogEntry {
            id: LogId::new(),
            date: day(date),
            job,
            plants: plants.to_vec(),
            plant_names: plants
                .iter()
                .map(|p| {
                    fx.doc
                        .object(*p)
                        .map(|o| o.name.clone())
                        .unwrap_or_default()
                })
                .collect(),
            product: sprayed.then_some(fx.copper),
            product_name: if sprayed {
                "Copper".into()
            } else {
                String::new()
            },
            amount: (job == Job::Harvested).then_some(Amount {
                value: 3.0,
                unit: AmountUnit::Kg,
            }),
            note: String::new(),
            created_ms: created,
        }
    }

    fn spray(fx: &mut Fx, date: &str) -> LogEntry {
        let a = fx.a;
        let e = entry(fx, Job::Sprayed, date, &[a], fx.doc.log.len() as i64);
        fx.doc.log.push(e.clone());
        e
    }

    #[test]
    fn safe_date_follows_latest_spray() {
        let mut fx = fixture();
        assert_eq!(safe_to_harvest(&fx.doc, fx.a), None);
        spray(&mut fx, "2024-06-01");
        assert_eq!(safe_to_harvest(&fx.doc, fx.a), Some(day("2024-06-22")));
        spray(&mut fx, "2024-06-20");
        assert_eq!(safe_to_harvest(&fx.doc, fx.a), Some(day("2024-07-11")));
        assert_eq!(safe_to_harvest(&fx.doc, fx.b), None);
        assert_eq!(
            safe_to_harvest_on(&fx.doc, fx.a, day("2024-06-10")),
            Some(day("2024-06-22"))
        );
        fx.doc.products[0].harvest_days = None;
        assert_eq!(safe_to_harvest(&fx.doc, fx.a), None);
    }

    #[test]
    fn not_before_only_in_future() {
        let mut fx = fixture();
        spray(&mut fx, "2024-06-01");
        assert_eq!(
            not_before(&fx.doc, fx.a, day("2024-06-10")),
            vec![NotBefore {
                product: fx.copper,
                name: "Copper".into(),
                date: day("2024-06-15")
            }]
        );
        assert!(not_before(&fx.doc, fx.a, day("2024-06-15")).is_empty());
        assert!(not_before(&fx.doc, fx.b, day("2024-06-10")).is_empty());
    }

    #[test]
    fn back_filled_spray_warns_both_ways() {
        let mut fx = fixture();
        spray(&mut fx, "2024-06-01");
        spray(&mut fx, "2024-06-20");
        let draft = entry(&fx, Job::Sprayed, "2024-06-10", &[fx.a], 99);
        let w = entry_warnings(&fx.doc, &draft);
        assert!(
            w.iter().any(|w| matches!(w, Warning::TooSoon { .. })),
            "{w:?}"
        );
        // 14 days after 06-01 is fine; 5 days before 06-20 is not
        let draft = entry(&fx, Job::Sprayed, "2024-06-15", &[fx.a], 99);
        assert_eq!(
            entry_warnings(&fx.doc, &draft),
            vec![Warning::TooSoon {
                plant: fx.a,
                product: "Copper".into(),
                other: day("2024-06-20"),
                days: 14
            }]
        );
        // only 06-01 saved: 06-15 is exactly 14 days later, fine
        fx.doc.log.pop();
        assert!(entry_warnings(&fx.doc, &draft).is_empty());
        // plant B has no sprays: no warning for it
        let both = entry(&fx, Job::Sprayed, "2024-06-05", &[fx.a, fx.b], 99);
        let w = entry_warnings(&fx.doc, &both);
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn season_is_the_calendar_year() {
        let mut fx = fixture();
        spray(&mut fx, "2024-01-10");
        spray(&mut fx, "2024-05-10");
        spray(&mut fx, "2024-12-01");
        assert_eq!(season_count(&fx.doc, fx.a, fx.copper, 2024), 3);
        let late = entry(&fx, Job::Sprayed, "2024-12-20", &[fx.a], 99);
        assert!(
            entry_warnings(&fx.doc, &late).contains(&Warning::OverSeason {
                plant: fx.a,
                product: "Copper".into(),
                year: 2024,
                count: 4,
                max: 3
            })
        );
        let next_year = entry(&fx, Job::Sprayed, "2025-01-02", &[fx.a], 99);
        assert!(
            !entry_warnings(&fx.doc, &next_year)
                .iter()
                .any(|w| matches!(w, Warning::OverSeason { .. }))
        );
    }

    #[test]
    fn harvest_too_early() {
        let mut fx = fixture();
        spray(&mut fx, "2024-06-01");
        let early = entry(&fx, Job::Harvested, "2024-06-10", &[fx.a], 99);
        assert_eq!(
            entry_warnings(&fx.doc, &early),
            vec![Warning::HarvestTooEarly {
                plant: fx.a,
                safe: day("2024-06-22")
            }]
        );
        let ok = entry(&fx, Job::Harvested, "2024-06-22", &[fx.a], 99);
        assert!(entry_warnings(&fx.doc, &ok).is_empty());
        // a spray after the harvest doesn't count against it
        let before = entry(&fx, Job::Harvested, "2024-05-20", &[fx.a], 99);
        assert!(entry_warnings(&fx.doc, &before).is_empty());
    }

    #[test]
    fn deleted_product_stops_counting() {
        let mut fx = fixture();
        let saved = spray(&mut fx, "2024-06-01");
        fx.doc
            .apply(Command::RemoveProduct { id: fx.copper }, 1)
            .unwrap();
        assert_eq!(safe_to_harvest(&fx.doc, fx.a), None);
        let again = entry(&fx, Job::Sprayed, "2024-06-02", &[fx.a], 99);
        assert!(entry_warnings(&fx.doc, &again).is_empty());
        assert!(not_before(&fx.doc, fx.a, day("2024-06-02")).is_empty());
        assert_eq!(history(&fx.doc, fx.a)[0].product_name, saved.product_name);
    }

    #[test]
    fn editing_an_entry_rechecks() {
        let mut fx = fixture();
        spray(&mut fx, "2024-06-01");
        let mut second = spray(&mut fx, "2024-06-05");
        assert_eq!(entry_warnings(&fx.doc, &second).len(), 1);
        second.date = day("2024-06-20");
        assert!(entry_warnings(&fx.doc, &second).is_empty());
    }

    #[test]
    fn history_newest_first() {
        let mut fx = fixture();
        let x = spray(&mut fx, "2024-06-10");
        let y = spray(&mut fx, "2024-06-01");
        let z = spray(&mut fx, "2024-06-10");
        let ids: Vec<LogId> = history(&fx.doc, fx.a).iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![z.id, x.id, y.id]);
        assert!(history(&fx.doc, fx.b).is_empty());
    }

    #[test]
    fn marks_and_group() {
        let mut fx = fixture();
        spray(&mut fx, "2024-06-01");
        let b = fx.b;
        let e = entry(&fx, Job::Sprayed, "2024-06-05", &[b], 50);
        fx.doc.log.push(e);
        fx.doc.care.insert(
            fx.b,
            PlantCare {
                health: Health::Gone,
                ..PlantCare::default()
            },
        );
        let marks = garden_marks(&fx.doc, day("2024-06-10"));
        assert_eq!(
            marks,
            vec![
                GardenMark {
                    id: fx.a,
                    health: Health::Healthy,
                    not_safe: true
                },
                GardenMark {
                    id: fx.b,
                    health: Health::Gone,
                    not_safe: true
                },
            ]
        );
        assert!(
            garden_marks(&fx.doc, day("2024-06-26"))
                .iter()
                .all(|m| !m.not_safe)
        );
        assert_eq!(
            group_safe_to_harvest(&fx.doc, &[fx.a, fx.b]),
            Some(day("2024-06-22"))
        );
    }

    #[test]
    fn names_and_text() {
        let mut fx = fixture();
        assert_eq!(date_label(day("2024-06-03")), "3 Jun 2024");
        let e = spray(&mut fx, "2024-06-01");
        let w = Warning::TooSoon {
            plant: fx.a,
            product: "Copper".into(),
            other: e.date,
            days: 14,
        };
        assert_eq!(
            warning_text(&fx.doc, &w),
            "Copper was also applied to Apple A on 1 Jun 2024. The label allows one application every 14 days."
        );
        fx.doc.apply(Command::Delete { id: fx.a }, 1).unwrap();
        assert_eq!(plant_name(&fx.doc, fx.a), "Apple A (removed from plan)");
        assert_eq!(plant_name(&fx.doc, fx.b), "Apple B");
    }
}
