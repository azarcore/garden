//! Built-in plant catalog. Sizes are typical mature spread and height in mm.

use garden_core::model::{PlantCategory as C, PlantDef, PlantSymbol as S};

pub fn builtin_plants() -> Vec<PlantDef> {
    let rows: [(&str, &str, C, f64, f64, S); 20] = [
        ("oak", "Oak Tree", C::Tree, 12000., 15000., S::Round),
        ("birch", "Silver Birch", C::Tree, 5000., 12000., S::Leafy),
        ("apple", "Apple Tree", C::Tree, 4000., 4000., S::Round),
        (
            "cherry",
            "Ornamental Cherry",
            C::Tree,
            5000.,
            6000.,
            S::Round,
        ),
        ("acer", "Japanese Maple", C::Tree, 3000., 3000., S::Leafy),
        ("yew", "Yew", C::Shrub, 3000., 4000., S::Conifer),
        ("box", "Box Ball", C::Shrub, 600., 600., S::Round),
        ("lavender", "Lavender", C::Shrub, 600., 500., S::Spiky),
        ("rose", "Shrub Rose", C::Shrub, 1200., 1200., S::Round),
        ("hydrangea", "Hydrangea", C::Shrub, 1500., 1500., S::Round),
        ("rosemary", "Rosemary", C::Shrub, 900., 1000., S::Spiky),
        ("hosta", "Hosta", C::Perennial, 800., 500., S::Leafy),
        ("salvia", "Salvia", C::Perennial, 500., 600., S::Spiky),
        (
            "geranium",
            "Hardy Geranium",
            C::Perennial,
            600.,
            400.,
            S::Round,
        ),
        ("grass", "Ornamental Grass", C::Grass, 800., 1200., S::Spiky),
        ("fern", "Fern", C::Perennial, 700., 600., S::Leafy),
        ("clematis", "Clematis", C::Climber, 1000., 3000., S::Star),
        ("tomato", "Tomato", C::Vegetable, 600., 1500., S::Leafy),
        ("lettuce", "Lettuce", C::Vegetable, 300., 250., S::Leafy),
        ("basil", "Basil", C::Herb, 300., 400., S::Leafy),
    ];
    rows.into_iter()
        .map(|(id, name, category, spread, height, symbol)| PlantDef {
            id: id.into(),
            name: name.into(),
            category,
            spread,
            height,
            symbol,
            custom: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_sane() {
        let c = builtin_plants();
        assert_eq!(c.len(), 20);
        let ids: std::collections::HashSet<_> = c.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids.len(), 20);
        assert!(
            c.iter()
                .all(|p| p.spread > 0. && p.height > 0. && !p.custom && !p.name.is_empty())
        );
    }
}
