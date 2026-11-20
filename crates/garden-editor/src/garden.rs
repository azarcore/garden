//! What the plant card, the job form and the products library can change.
//! Everything here works only in Garden mode.

use garden_core::command::Command;
use garden_core::date::Date;
use garden_core::model::{
    Amount, Health, Job, LogEntry, LogId, ObjectId, Photo, PlantCare, Product, ProductId,
    ProductKind,
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
        let mut plants = Vec::new();
        let mut plant_names = Vec::new();
        for id in &draft.plants {
            if let Some(o) = doc.object(*id)
                && is_gardenable(o)
                && self.care_of(*id).health != Health::Gone
                && !plants.contains(id)
            {
                plants.push(*id);
                plant_names.push(o.name.clone());
            }
        }
        if plants.is_empty() {
            return Err(LogError::NoPlants);
        }
        let product = draft
            .product
            .and_then(|id| doc.products.iter().find(|p| p.id == id));
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
        let saved = draft.id.and_then(|id| doc.log.iter().find(|e| e.id == id));
        Ok(LogEntry {
            id: draft.id.unwrap_or_else(LogId::new),
            date: draft.date,
            job: draft.job,
            plants,
            plant_names,
            product: product.map(|p| p.id),
            product_name: product.map(|p| p.name.clone()).unwrap_or_default(),
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
}
