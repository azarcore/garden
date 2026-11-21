//! What the plant card, the job form and the products library can change.
//! Everything here works only in Garden mode.

use garden_core::command::Command;
use garden_core::date::Date;
use garden_core::model::{
    Amount, CareTask, Health, Job, LogEntry, LogId, ObjectId, Photo, PlantCare, Product, ProductId,
    ProductKind, Repeat, Target, TaskId,
};

use crate::care::is_gardenable;
use crate::state::{EditorState, Mode};
use crate::strings::Strings;

/// A job as typed into the form. `id` is set when editing a saved entry.
#[derive(Clone, Debug, PartialEq)]
pub struct LogDraft {
    pub id: Option<LogId>,
    pub date: Date,
    pub job: Job,
    pub plants: Vec<ObjectId>,
    pub product: Option<ProductId>,
    pub amount: Option<Amount>,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogError {
    FutureDate,
    NoPlants,
    ProductRequired,
    AmountRequired,
    BadAmount,
}

impl LogError {
    pub fn message(self, s: &Strings) -> &'static str {
        match self {
            LogError::FutureDate => s.err_future_date,
            LogError::NoPlants => s.err_no_plants,
            LogError::ProductRequired => s.err_product_required,
            LogError::AmountRequired => s.err_amount_required,
            LogError::BadAmount => s.err_bad_amount,
        }
    }
}

/// A product as typed into the form. `id` is set when editing.
#[derive(Clone, Debug, PartialEq)]
pub struct ProductDraft {
    pub id: Option<ProductId>,
    pub name: String,
    pub kind: ProductKind,
    pub harvest_days: Option<u32>,
    pub reapply_days: Option<u32>,
    pub max_per_season: Option<u32>,
    pub notes: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductError {
    NoName,
    ZeroInterval,
}

impl ProductError {
    pub fn message(self, s: &Strings) -> &'static str {
        match self {
            ProductError::NoName => s.err_product_name,
            ProductError::ZeroInterval => s.err_bad_days,
        }
    }
}

/// A repeating job as typed into its form. `id` is set when editing.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskDraft {
    pub id: Option<TaskId>,
    pub job: Job,
    pub product: Option<ProductId>,
    pub note: String,
    pub target: Target,
    pub repeat: Repeat,
    pub months: Option<(u32, u32)>,
    pub first_due: Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskError {
    NoPlants,
    ProductRequired,
    BadRepeat,
    BadMonths,
}

impl TaskError {
    pub fn message(self, s: &Strings) -> &'static str {
        match self {
            TaskError::NoPlants => s.err_task_no_plants,
            TaskError::ProductRequired => s.err_product_required,
            TaskError::BadRepeat => s.err_task_repeat,
            TaskError::BadMonths => s.err_task_months,
        }
    }
}

/// A days field: empty means not set, otherwise a whole number ≥ 0.
#[allow(clippy::result_unit_err)]
pub fn parse_days(s: &str) -> Result<Option<u32>, ()> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err(());
    }
    s.parse().map(Some).map_err(|_| ())
}

impl EditorState {
    /// Save a new or edited repeating job (Garden mode). Editing keeps its skips.
    pub fn save_task(&mut self, d: TaskDraft, now_ms: i64) -> Result<TaskId, TaskError> {
        if matches!(&d.target, Target::Plants { ids } if ids.is_empty()) {
            return Err(TaskError::NoPlants);
        }
        if let Repeat::Every { days } = d.repeat
            && !(1..=2555).contains(&days)
        {
            return Err(TaskError::BadRepeat);
        }
        if d.months
            .is_some_and(|(a, b)| !(1..=12).contains(&a) || !(1..=12).contains(&b))
        {
            return Err(TaskError::BadMonths);
        }
        let product = d
            .product
            .and_then(|id| self.doc().products.iter().find(|p| p.id == id));
        if d.job == Job::Sprayed && product.is_none() {
            return Err(TaskError::ProductRequired);
        }
        if !self.in_garden() {
            return Err(TaskError::NoPlants);
        }
        let saved =
            d.id.and_then(|id| self.doc().tasks.iter().find(|t| t.id == id));
        let task = CareTask {
            id: d.id.unwrap_or_else(TaskId::new),
            job: d.job,
            product: product.map(|p| p.id),
            product_name: product.map(|p| p.name.clone()).unwrap_or_default(),
            note: d.note.trim().to_string(),
            target: d.target,
            repeat: d.repeat,
            months: d.months,
            first_due: d.first_due,
            skips: saved.map(|t| t.skips.clone()).unwrap_or_default(),
        };
        let id = task.id;
        if saved != Some(&task) {
            self.execute(Command::UpsertTask { task }, now_ms);
        }
        Ok(id)
    }

    pub fn remove_task(&mut self, id: TaskId, now_ms: i64) {
        if self.in_garden() && self.doc().tasks.iter().any(|t| t.id == id) {
            self.execute(Command::RemoveTask { id }, now_ms);
        }
    }

    /// Record each plant's current due date as skipped, as one undo step.
    pub fn skip_task(&mut self, id: TaskId, plants: &[ObjectId], now_ms: i64) {
        if !self.in_garden() || plants.is_empty() {
            return;
        }
        let Some(mut task) = self.doc().tasks.iter().find(|t| t.id == id).cloned() else {
            return;
        };
        for p in plants {
            let due = crate::calendar::plant_due(self.doc(), &task, *p).date;
            task.skips.insert(*p, due);
        }
        self.execute(Command::UpsertTask { task }, now_ms);
    }

    fn in_garden(&self) -> bool {
        self.mode == Mode::Garden
    }

    /// The care record of a plant (a healthy, empty one if there is none).
    pub fn care_of(&self, id: ObjectId) -> PlantCare {
        self.doc().care.get(&id).cloned().unwrap_or_default()
    }

    fn set_care(&mut self, id: ObjectId, care: PlantCare, now_ms: i64) {
        if self.in_garden() && care != self.care_of(id) {
            self.execute(
                Command::SetCare {
                    id,
                    care: Some(care),
                },
                now_ms,
            );
        }
    }

    /// Set the health of several plants as one undo step.
    pub fn set_health(&mut self, ids: &[ObjectId], health: Health, now_ms: i64) {
        if !self.in_garden() {
            return;
        }
        let commands: Vec<Command> = ids
            .iter()
            .filter_map(|id| {
                let mut care = self.care_of(*id);
                (care.health != health).then(|| {
                    care.health = health;
                    Command::SetCare {
                        id: *id,
                        care: Some(care),
                    }
                })
            })
            .collect();
        if !commands.is_empty() {
            self.execute(Command::Batch { commands }, now_ms);
        }
    }

    /// Set a plant's notes. The same text again is not another undo step.
    pub fn set_notes(&mut self, id: ObjectId, notes: &str, now_ms: i64) {
        let mut care = self.care_of(id);
        care.notes = notes.trim_end().to_string();
        self.set_care(id, care, now_ms);
    }

    pub fn add_photo(&mut self, id: ObjectId, photo: Photo, now_ms: i64) {
        let mut care = self.care_of(id);
        care.photos.push(photo);
        self.set_care(id, care, now_ms);
    }

    pub fn remove_photo(&mut self, id: ObjectId, image_ref: &str, now_ms: i64) {
        let mut care = self.care_of(id);
        care.photos.retain(|p| p.image_ref != image_ref);
        self.set_care(id, care, now_ms);
    }

    /// The entry a draft would save, for showing warnings before saving.
    /// Gone plants and anything that isn't a plant or hedge are left out.
    pub fn draft_entry(&self, draft: &LogDraft, now_ms: i64) -> Result<LogEntry, LogError> {
        let doc = self.doc();
        // when editing, plants and the product already in the entry stay, even
        // if they have since gone, been removed from the plan or deleted
        let saved = draft.id.and_then(|id| doc.log.iter().find(|e| e.id == id));
        let mut plants = Vec::new();
        let mut plant_names = Vec::new();
        for id in &draft.plants {
            if plants.contains(id) {
                continue;
            }
            let kept = saved.and_then(|e| {
                let i = e.plants.iter().position(|p| p == id)?;
                e.plant_names.get(i).cloned()
            });
            let current = doc.object(*id).filter(|o| is_gardenable(o));
            let name = match (current, kept) {
                (Some(o), Some(_)) => Some(o.name.clone()),
                (None, Some(saved_name)) => Some(saved_name),
                (Some(o), None) if self.care_of(*id).health != Health::Gone => Some(o.name.clone()),
                _ => None,
            };
            if let Some(name) = name {
                plants.push(*id);
                plant_names.push(name);
            }
        }
        if plants.is_empty() {
            return Err(LogError::NoPlants);
        }
        let product = draft
            .product
            .and_then(|id| doc.products.iter().find(|p| p.id == id))
            .map(|p| (p.id, p.name.clone()))
            .or_else(|| {
                let e = saved?;
                let id = e.product.filter(|p| Some(*p) == draft.product)?;
                Some((id, e.product_name.clone()))
            });
        if draft.job == Job::Sprayed && product.is_none() {
            return Err(LogError::ProductRequired);
        }
        if let Some(a) = draft.amount
            && !(a.value.is_finite() && a.value > 0.0)
        {
            return Err(LogError::BadAmount);
        }
        if draft.job == Job::Harvested && draft.amount.is_none() {
            return Err(LogError::AmountRequired);
        }
        let (product, product_name) = product.unzip();
        Ok(LogEntry {
            id: draft.id.unwrap_or_else(LogId::new),
            date: draft.date,
            job: draft.job,
            plants,
            plant_names,
            product,
            product_name: product_name.unwrap_or_default(),
            amount: draft.amount,
            note: draft.note.trim().to_string(),
            created_ms: saved.map_or(now_ms, |e| e.created_ms),
        })
    }

    /// Save a new or edited entry. Days after `today` are refused.
    pub fn save_entry(
        &mut self,
        draft: LogDraft,
        today: Date,
        now_ms: i64,
    ) -> Result<LogId, LogError> {
        if !self.in_garden() {
            return Err(LogError::NoPlants);
        }
        if draft.date > today {
            return Err(LogError::FutureDate);
        }
        let entry = self.draft_entry(&draft, now_ms)?;
        let id = entry.id;
        self.execute(Command::UpsertLogEntry { entry }, now_ms);
        Ok(id)
    }

    pub fn remove_entry(&mut self, id: LogId, now_ms: i64) {
        if self.in_garden() && self.doc().log.iter().any(|e| e.id == id) {
            self.execute(Command::RemoveLogEntry { id }, now_ms);
        }
    }

    pub fn save_product(
        &mut self,
        draft: ProductDraft,
        now_ms: i64,
    ) -> Result<ProductId, ProductError> {
        let name = draft.name.trim();
        if name.is_empty() {
            return Err(ProductError::NoName);
        }
        if draft.reapply_days == Some(0) || draft.max_per_season == Some(0) {
            return Err(ProductError::ZeroInterval);
        }
        if !self.in_garden() {
            return Err(ProductError::NoName);
        }
        let product = Product {
            id: draft.id.unwrap_or_else(ProductId::new),
            name: name.to_string(),
            kind: draft.kind,
            harvest_days: draft.harvest_days,
            reapply_days: draft.reapply_days,
            max_per_season: draft.max_per_season,
            notes: draft.notes.trim().to_string(),
        };
        let id = product.id;
        if self.doc().products.contains(&product) {
            return Ok(id);
        }
        self.execute(Command::UpsertProduct { product }, now_ms);
        Ok(id)
    }

    pub fn remove_product(&mut self, id: ProductId, now_ms: i64) {
        if self.in_garden() && self.doc().products.iter().any(|p| p.id == id) {
            self.execute(Command::RemoveProduct { id }, now_ms);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{plant_at, state_with};
    use garden_core::model::{AmountUnit, ProductKind};

    fn day(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn garden() -> (EditorState, ObjectId, ObjectId) {
        let mut s = state_with(vec![
            plant_at(0., 0., 3000.),
            plant_at(5000., 0., 3000.),
            crate::test_util::lawn_rect(),
        ]);
        s.set_mode(Mode::Garden);
        let (a, b) = (s.doc().objects[0].id, s.doc().objects[1].id);
        (s, a, b)
    }

    fn copper() -> ProductDraft {
        ProductDraft {
            id: None,
            name: " Copper ".into(),
            kind: ProductKind::Fungicide,
            harvest_days: Some(21),
            reapply_days: Some(14),
            max_per_season: Some(3),
            notes: String::new(),
        }
    }

    fn draft(job: Job, plants: &[ObjectId]) -> LogDraft {
        LogDraft {
            id: None,
            date: day("2024-06-01"),
            job,
            plants: plants.to_vec(),
            product: None,
            amount: None,
            note: String::new(),
        }
    }

    #[test]
    fn health_for_many_is_one_undo() {
        let (mut s, a, b) = garden();
        s.set_health(&[a, b], Health::Sick, 1);
        assert_eq!(s.care_of(a).health, Health::Sick);
        assert_eq!(s.care_of(b).health, Health::Sick);
        assert!(s.undo(2));
        assert_eq!(s.care_of(a).health, Health::Healthy);
        assert_eq!(s.care_of(b).health, Health::Healthy);
        assert!(s.doc().care.is_empty());
    }

    #[test]
    fn notes_skip_repeats() {
        let (mut s, a, _) = garden();
        s.set_notes(a, "aphids on new growth", 1);
        let n = s.change_count();
        s.set_notes(a, "aphids on new growth", 2);
        assert_eq!(s.change_count(), n);
        assert_eq!(s.care_of(a).notes, "aphids on new growth");
        assert!(s.undo(3));
        assert_eq!(s.care_of(a).notes, "");
    }

    #[test]
    fn photo_add_remove_undo() {
        let (mut s, a, _) = garden();
        let photo = Photo {
            image_ref: "photo-1".into(),
            date: day("2024-06-01"),
            added_ms: 1,
        };
        s.add_photo(a, photo.clone(), 1);
        assert_eq!(s.care_of(a).photos, vec![photo.clone()]);
        s.remove_photo(a, "photo-1", 2);
        assert!(s.care_of(a).photos.is_empty());
        assert!(s.undo(3));
        assert_eq!(s.care_of(a).photos, vec![photo]);
    }

    #[test]
    fn log_validation() {
        let (mut s, a, b) = garden();
        let lawn = s.doc().objects[2].id;
        let today = day("2024-06-10");
        assert_eq!(
            s.save_entry(draft(Job::Pruned, &[]), today, 1),
            Err(LogError::NoPlants)
        );
        assert_eq!(
            s.save_entry(draft(Job::Pruned, &[lawn]), today, 1),
            Err(LogError::NoPlants)
        );
        assert_eq!(
            s.save_entry(draft(Job::Sprayed, &[a]), today, 1),
            Err(LogError::ProductRequired)
        );
        assert_eq!(
            s.save_entry(draft(Job::Harvested, &[a]), today, 1),
            Err(LogError::AmountRequired)
        );
        let mut bad = draft(Job::Harvested, &[a]);
        bad.amount = Some(Amount {
            value: -1.0,
            unit: AmountUnit::Kg,
        });
        assert_eq!(s.save_entry(bad, today, 1), Err(LogError::BadAmount));
        // a gone plant is dropped from a two-plant job
        s.set_health(&[b], Health::Gone, 1);
        let id = s.save_entry(draft(Job::Pruned, &[a, b]), today, 2).unwrap();
        let e = s.doc().log.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.plants, vec![a]);
        assert_eq!(e.plant_names, vec![s.doc().objects[0].name.clone()]);
        assert_eq!(
            s.save_entry(draft(Job::Pruned, &[b]), today, 3),
            Err(LogError::NoPlants)
        );
        // a spray takes the product's name
        let p = s.save_product(copper(), 4).unwrap();
        let mut sp = draft(Job::Sprayed, &[a]);
        sp.product = Some(p);
        let id = s.save_entry(sp, today, 5).unwrap();
        let e = s.doc().log.iter().find(|e| e.id == id).unwrap();
        assert_eq!(e.product_name, "Copper");
    }

    #[test]
    fn editing_keeps_created_time() {
        let (mut s, a, _) = garden();
        let today = day("2024-06-10");
        let id = s.save_entry(draft(Job::Watered, &[a]), today, 100).unwrap();
        let mut edit = draft(Job::Watered, &[a]);
        edit.id = Some(id);
        edit.note = "deep soak".into();
        assert_eq!(s.save_entry(edit, today, 500), Ok(id));
        assert_eq!(s.doc().log.len(), 1);
        assert_eq!(s.doc().log[0].created_ms, 100);
        assert_eq!(s.doc().log[0].note, "deep soak");
        s.remove_entry(id, 600);
        assert!(s.doc().log.is_empty());
        assert!(s.undo(700));
        assert_eq!(s.doc().log[0].note, "deep soak");
    }

    #[test]
    fn future_dates_refused() {
        let (mut s, a, _) = garden();
        let mut d = draft(Job::Watered, &[a]);
        d.date = day("2024-06-11");
        assert_eq!(
            s.save_entry(d.clone(), day("2024-06-10"), 1),
            Err(LogError::FutureDate)
        );
        assert!(s.save_entry(d, day("2024-06-11"), 1).is_ok());
    }

    #[test]
    fn product_validation_and_parse_days() {
        let (mut s, _, _) = garden();
        let mut blank = copper();
        blank.name = "  ".into();
        assert_eq!(s.save_product(blank, 1), Err(ProductError::NoName));
        let mut zero = copper();
        zero.reapply_days = Some(0);
        assert_eq!(s.save_product(zero, 1), Err(ProductError::ZeroInterval));
        let mut zero = copper();
        zero.max_per_season = Some(0);
        assert_eq!(s.save_product(zero, 1), Err(ProductError::ZeroInterval));
        let id = s.save_product(copper(), 2).unwrap();
        assert_eq!(s.doc().products[0].name, "Copper");
        let mut edit = copper();
        edit.id = Some(id);
        edit.harvest_days = Some(0);
        assert_eq!(s.save_product(edit, 3), Ok(id));
        assert_eq!(s.doc().products.len(), 1);
        assert_eq!(s.doc().products[0].harvest_days, Some(0));
        s.remove_product(id, 4);
        assert!(s.doc().products.is_empty());
        assert_eq!(parse_days(""), Ok(None));
        assert_eq!(parse_days(" 14 "), Ok(Some(14)));
        assert_eq!(parse_days("0"), Ok(Some(0)));
        for bad in ["-1", "1.5", "x", "1e3"] {
            assert_eq!(parse_days(bad), Err(()), "{bad}");
        }
    }

    #[test]
    fn design_mode_refuses_care_edits() {
        let (mut s, a, _) = garden();
        s.set_mode(Mode::Design);
        let before = s.doc().clone();
        s.set_health(&[a], Health::Sick, 1);
        s.set_notes(a, "x", 1);
        assert!(s.save_product(copper(), 1).is_err());
        assert!(
            s.save_entry(draft(Job::Watered, &[a]), day("2024-06-10"), 1)
                .is_err()
        );
        assert_eq!(s.doc(), &before);
    }

    #[test]
    fn editing_keeps_gone_and_removed_plants() {
        let (mut s, a, b) = garden();
        let today = day("2024-06-10");
        let p = s.save_product(copper(), 1).unwrap();
        let mut fed = draft(Job::Fed, &[a, b]);
        fed.product = Some(p);
        let id = s.save_entry(fed.clone(), today, 2).unwrap();
        // B goes, then is deleted in Design; the product is deleted too
        s.set_health(&[b], Health::Gone, 3);
        s.remove_product(p, 4);
        s.set_mode(Mode::Design);
        s.try_execute(Command::Delete { id: b }, 5).unwrap();
        s.set_mode(Mode::Garden);
        let mut edit = fed.clone();
        edit.id = Some(id);
        edit.note = "fixed typo".into();
        assert_eq!(s.save_entry(edit, today, 6), Ok(id));
        let e = &s.doc().log[0];
        assert_eq!(e.plants, vec![a, b]);
        assert_eq!(e.plant_names.len(), 2);
        assert_eq!(e.product, Some(p));
        assert_eq!(e.product_name, "Copper");
        assert_eq!(e.note, "fixed typo");
        // an entry that only covers a gone plant can still be edited
        let (mut s, a, _) = garden();
        let id = s.save_entry(draft(Job::Pruned, &[a]), today, 1).unwrap();
        s.set_health(&[a], Health::Gone, 2);
        let mut edit = draft(Job::Pruned, &[a]);
        edit.id = Some(id);
        edit.note = "late note".into();
        assert_eq!(s.save_entry(edit, today, 3), Ok(id));
    }

    #[test]
    fn garden_mode_keeps_name_and_list_edits() {
        let (mut s, _, _) = garden();
        s.rename("Back garden", 1);
        s.set_price("area:lawn", Some(2.0), 2);
        s.set_currency("€", 3);
        s.set_gravel_depth(80.0, 4);
        assert_eq!(s.doc().name, "Back garden");
        assert_eq!(s.doc().prices.get("area:lawn"), Some(&2.0));
        assert_eq!(s.doc().currency, "€");
        assert_eq!(s.doc().gravel_depth, Some(80.0));
        assert!(s.can_undo());
        assert!(s.undo(5));
        assert_eq!(s.doc().gravel_depth, None);
    }

    fn feed_every(days: u32, target: Target) -> TaskDraft {
        TaskDraft {
            id: None,
            job: Job::Fed,
            product: None,
            note: "half dose".into(),
            target,
            repeat: Repeat::Every { days },
            months: None,
            first_due: day("2026-06-01"),
        }
    }

    #[test]
    fn task_validation() {
        let (mut s, a, _) = garden();
        let none = Target::Plants { ids: vec![] };
        assert_eq!(
            s.save_task(feed_every(7, none), 1),
            Err(TaskError::NoPlants)
        );
        let one = Target::Plants { ids: vec![a] };
        assert_eq!(
            s.save_task(feed_every(0, one.clone()), 1),
            Err(TaskError::BadRepeat)
        );
        assert_eq!(
            s.save_task(feed_every(2556, one.clone()), 1),
            Err(TaskError::BadRepeat)
        );
        let mut months = feed_every(7, one.clone());
        months.months = Some((0, 4));
        assert_eq!(s.save_task(months, 1), Err(TaskError::BadMonths));
        let mut spray = feed_every(7, one.clone());
        spray.job = Job::Sprayed;
        assert_eq!(
            s.save_task(spray.clone(), 1),
            Err(TaskError::ProductRequired)
        );
        // a kind with no plants on the plan is allowed
        let kind = Target::Kind {
            catalog_id: "quince".into(),
            name: "Quince".into(),
        };
        assert!(s.save_task(feed_every(7, kind), 2).is_ok());
        // a spray takes its product's name
        let p = s.save_product(copper(), 3).unwrap();
        spray.product = Some(p);
        let id = s.save_task(spray, 4).unwrap();
        let t = s.doc().tasks.iter().find(|t| t.id == id).unwrap();
        assert_eq!(t.product_name, "Copper");
    }

    #[test]
    fn editing_keeps_skips() {
        let (mut s, a, _) = garden();
        let id = s
            .save_task(feed_every(28, Target::Plants { ids: vec![a] }), 1)
            .unwrap();
        s.skip_task(id, &[a], 2);
        let skips = s.doc().tasks[0].skips.clone();
        assert_eq!(skips.get(&a), Some(&day("2026-06-01")));
        let mut edit = feed_every(14, Target::Plants { ids: vec![a] });
        edit.id = Some(id);
        assert_eq!(s.save_task(edit, 3), Ok(id));
        assert_eq!(s.doc().tasks.len(), 1);
        assert_eq!(s.doc().tasks[0].skips, skips);
        assert_eq!(s.doc().tasks[0].repeat, Repeat::Every { days: 14 });
        s.remove_task(id, 4);
        assert!(s.doc().tasks.is_empty());
        assert!(s.undo(5));
        assert_eq!(s.doc().tasks.len(), 1);
    }

    #[test]
    fn skip_moves_on_and_undoes() {
        use crate::calendar::plant_due;
        let (mut s, a, b) = garden();
        let id = s
            .save_task(feed_every(28, Target::Plants { ids: vec![a, b] }), 1)
            .unwrap();
        let before = s.doc().tasks[0].clone();
        s.skip_task(id, &[a], 2);
        let t = s.doc().tasks[0].clone();
        assert_eq!(plant_due(s.doc(), &t, a).date, day("2026-06-29"));
        assert_eq!(plant_due(s.doc(), &t, b).date, day("2026-06-01"));
        assert!(s.undo(3));
        assert_eq!(s.doc().tasks[0], before);
        let t = s.doc().tasks[0].clone();
        assert_eq!(plant_due(s.doc(), &t, a).date, day("2026-06-01"));
    }

    #[test]
    fn design_mode_refuses_task_edits() {
        let (mut s, a, _) = garden();
        let id = s
            .save_task(feed_every(28, Target::Plants { ids: vec![a] }), 1)
            .unwrap();
        s.set_mode(Mode::Design);
        let before = s.doc().clone();
        assert!(
            s.save_task(feed_every(7, Target::Plants { ids: vec![a] }), 2)
                .is_err()
        );
        s.skip_task(id, &[a], 3);
        s.remove_task(id, 4);
        assert_eq!(s.doc(), &before);
    }
}
