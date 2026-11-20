//! Your own plants: the New plant form's rules, which custom plants a design
//! uses, and merging plants that come in with an imported design.

use garden_core::model::{Document, ObjectKind, PlantCategory, PlantDef, PlantSymbol};
use garden_core::units::{UnitSystem, format_length, parse_length};

use crate::strings::Strings;

/// Longest plant name accepted (characters).
const MAX_NAME_CHARS: usize = 60;

/// What the New plant / Edit plant form holds, as typed.
#[derive(Clone, Debug, PartialEq)]
pub struct PlantForm {
    pub name: String,
    pub category: PlantCategory,
    pub symbol: PlantSymbol,
    pub spread: String,
    pub height: String,
    /// Comma-separated other names.
    pub aliases: String,
}

impl Default for PlantForm {
    fn default() -> Self {
        PlantForm {
            name: String::new(),
            category: PlantCategory::Perennial,
            symbol: PlantSymbol::Round,
            spread: String::new(),
            height: String::new(),
            aliases: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormError {
    NameMissing,
    NameTooLong,
    SpreadInvalid,
    HeightInvalid,
}

impl FormError {
    pub fn message(self, s: &Strings) -> &'static str {
        match self {
            FormError::NameMissing => s.err_name_missing,
            FormError::NameTooLong => s.err_name_too_long,
            FormError::SpreadInvalid => s.err_spread_invalid,
            FormError::HeightInvalid => s.err_height_invalid,
        }
    }
}

fn positive_length(text: &str, units: UnitSystem) -> Option<f64> {
    parse_length(text, units)
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// A custom plant from the form; `id` is the plant's id when editing, None
/// for a new one.
pub fn plant_from_form(
    form: &PlantForm,
    id: Option<&str>,
    units: UnitSystem,
) -> Result<PlantDef, FormError> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(FormError::NameMissing);
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(FormError::NameTooLong);
    }
    let spread = positive_length(&form.spread, units).ok_or(FormError::SpreadInvalid)?;
    let height = positive_length(&form.height, units).ok_or(FormError::HeightInvalid)?;
    let aliases = form
        .aliases
        .split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_string)
        .collect();
    Ok(PlantDef {
        id: id.map_or_else(
            || format!("custom-{}", uuid::Uuid::new_v4()),
            str::to_string,
        ),
        name: name.to_string(),
        category: form.category,
        spread,
        height,
        symbol: form.symbol,
        custom: true,
        aliases,
    })
}

/// The form filled in from an existing plant (for editing).
pub fn form_from_plant(p: &PlantDef, units: UnitSystem) -> PlantForm {
    PlantForm {
        name: p.name.clone(),
        category: p.category,
        symbol: p.symbol,
        spread: format_length(p.spread, units),
        height: format_length(p.height, units),
        aliases: p.aliases.join(", "),
    }
}

/// Custom plants this design's plants were placed from (by catalog id), in
/// library order.
pub fn plants_used(doc: &Document, library: &[PlantDef]) -> Vec<PlantDef> {
    let used: Vec<&str> = doc
        .objects
        .iter()
        .filter_map(|o| match &o.kind {
            ObjectKind::Plant {
                catalog_id: Some(id),
                ..
            } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    library
        .iter()
        .filter(|p| used.contains(&p.id.as_str()))
        .cloned()
        .collect()
}

/// Imported plants the library doesn't have yet; a plant whose id is already
/// in the library is skipped, so someone's own edits are never overwritten.
pub fn new_from_import(library: &[PlantDef], imported: &[PlantDef]) -> Vec<PlantDef> {
    let mut out: Vec<PlantDef> = Vec::new();
    for p in imported {
        if !library.iter().any(|l| l.id == p.id) && !out.iter().any(|o| o.id == p.id) {
            out.push(p.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Group, catalog_with};
    use garden_core::command::Command;
    use garden_core::geometry::Point;
    use garden_core::model::{Document, PlantCategory, PlantSymbol};
    use garden_core::units::UnitSystem;

    fn form(name: &str, spread: &str, height: &str) -> PlantForm {
        PlantForm {
            name: name.into(),
            category: PlantCategory::Perennial,
            symbol: PlantSymbol::Spiky,
            spread: spread.into(),
            height: height.into(),
            aliases: " lupine, Russell lupin ,".into(),
        }
    }

    #[test]
    fn a_valid_form_makes_a_custom_plant() {
        let p = plant_from_form(
            &form("  Blue Lupin ", "45 cm", "0.9 m"),
            None,
            UnitSystem::Metric,
        )
        .unwrap();
        assert_eq!(p.name, "Blue Lupin");
        assert_eq!((p.spread, p.height), (450., 900.));
        assert!(p.custom && p.id.starts_with("custom-"));
        assert_eq!(p.aliases, ["lupine", "Russell lupin"]);
        let edited = plant_from_form(
            &form("Lupin", "50 cm", "1 m"),
            Some(&p.id),
            UnitSystem::Metric,
        )
        .unwrap();
        assert_eq!(edited.id, p.id);
        assert_eq!(form_from_plant(&p, UnitSystem::Metric).spread, "45 cm");
        assert_eq!(
            form_from_plant(&p, UnitSystem::Metric).aliases,
            "lupine, Russell lupin"
        );
    }

    #[test]
    fn bad_forms_say_what_is_wrong() {
        let m = UnitSystem::Metric;
        assert_eq!(
            plant_from_form(&form("  ", "1 m", "1 m"), None, m),
            Err(FormError::NameMissing)
        );
        assert_eq!(
            plant_from_form(&form(&"x".repeat(61), "1 m", "1 m"), None, m),
            Err(FormError::NameTooLong)
        );
        for bad in ["abc", "0", "-1 m", ""] {
            assert_eq!(
                plant_from_form(&form("A", bad, "1 m"), None, m),
                Err(FormError::SpreadInvalid),
                "{bad}"
            );
        }
        assert_eq!(
            plant_from_form(&form("A", "1 m", "0"), None, m),
            Err(FormError::HeightInvalid)
        );
        assert!(plant_from_form(&form("A", "2'6\"", "3'"), None, UnitSystem::Imperial).is_ok());
    }

    #[test]
    fn custom_plants_join_their_group_and_search() {
        let p = plant_from_form(
            &form("Blue Lupin", "45 cm", "90 cm"),
            None,
            UnitSystem::Metric,
        )
        .unwrap();
        let c = catalog_with(std::slice::from_ref(&p));
        let last_perennial = c
            .iter()
            .rposition(|e| e.group == Group::Perennials)
            .unwrap();
        assert_eq!(c[last_perennial].id, p.id);
        let hits = crate::search::search(&c, "lupine", None);
        assert_eq!(c[hits[0]].id, p.id);
    }

    #[test]
    fn designs_carry_the_custom_plants_they_use() {
        let p = plant_from_form(
            &form("Blue Lupin", "45 cm", "90 cm"),
            None,
            UnitSystem::Metric,
        )
        .unwrap();
        let other =
            plant_from_form(&form("Unused", "45 cm", "90 cm"), None, UnitSystem::Metric).unwrap();
        let mut d = Document::new("g", 0);
        let l = d.layers[0].id;
        d.apply(
            Command::Insert {
                object: p.place(Point::ZERO, l),
                index: 0,
            },
            0,
        )
        .unwrap();
        assert_eq!(
            plants_used(&d, &[other.clone(), p.clone()]),
            vec![p.clone()]
        );
    }

    #[test]
    fn imports_never_overwrite_the_library() {
        let mine =
            plant_from_form(&form("Mine", "45 cm", "90 cm"), None, UnitSystem::Metric).unwrap();
        let mut theirs = mine.clone();
        theirs.name = "Theirs".into();
        let fresh =
            plant_from_form(&form("New", "45 cm", "90 cm"), None, UnitSystem::Metric).unwrap();
        assert_eq!(
            new_from_import(&[mine], &[theirs, fresh.clone()]),
            vec![fresh]
        );
    }
}
