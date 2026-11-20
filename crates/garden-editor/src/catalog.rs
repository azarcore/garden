//! Built-in catalog: plants (typical mature spread and height in mm, plus
//! other names people search for) and garden features.

use garden_core::model::{
    AreaType, LinearKind, PlantCategory as C, PlantDef, PlantSymbol as S, StructureKind,
};

use crate::strings::EN;
use crate::tools::{DrawTarget, LineTarget, ToolKind};

/// How the catalog is grouped (chips and headings), in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Trees,
    Shrubs,
    Perennials,
    Grasses,
    Climbers,
    Edibles,
    Features,
}

impl Group {
    pub const ALL: [Group; 7] = [
        Group::Trees,
        Group::Shrubs,
        Group::Perennials,
        Group::Grasses,
        Group::Climbers,
        Group::Edibles,
        Group::Features,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Trees => EN.grp_trees,
            Group::Shrubs => EN.grp_shrubs,
            Group::Perennials => EN.grp_perennials,
            Group::Grasses => EN.grp_grasses,
            Group::Climbers => EN.grp_climbers,
            Group::Edibles => EN.grp_edibles,
            Group::Features => EN.grp_features,
        }
    }
}

/// Garden features that are drawn rather than placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureId {
    Bed,
    Lawn,
    Paving,
    Gravel,
    Water,
    Path,
    Fence,
    Wall,
    Hedge,
    Shed,
    Greenhouse,
    Pergola,
    RaisedBed,
    Seating,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EntryKind {
    Plant(PlantDef),
    Feature(FeatureId),
}

/// One thing the catalog offers.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    /// Other names people search for.
    pub aliases: Vec<&'static str>,
    pub group: Group,
    pub kind: EntryKind,
}

pub fn group_of(category: C) -> Group {
    match category {
        C::Tree => Group::Trees,
        C::Shrub => Group::Shrubs,
        C::Perennial | C::Annual | C::Other => Group::Perennials,
        C::Grass => Group::Grasses,
        C::Climber => Group::Climbers,
        C::Vegetable | C::Herb => Group::Edibles,
    }
}

/// Height given to new structures of each kind (mm).
pub fn structure_height(kind: StructureKind) -> f64 {
    match kind {
        StructureKind::Shed => 2200.0,
        StructureKind::Greenhouse | StructureKind::Pergola => 2400.0,
        StructureKind::RaisedBed => 400.0,
        StructureKind::Furniture => 900.0,
        StructureKind::Other => 2000.0,
    }
}

/// The drawing tool (preset) for a feature.
pub fn feature_tool(f: FeatureId) -> ToolKind {
    let line = |kind, thickness, height| {
        ToolKind::Polyline(LineTarget::Linear {
            kind,
            thickness,
            height,
        })
    };
    let structure = |k| ToolKind::Rect(DrawTarget::Structure(k));
    match f {
        FeatureId::Bed => ToolKind::Polygon(DrawTarget::Area(AreaType::Bed)),
        FeatureId::Lawn => ToolKind::Polygon(DrawTarget::Area(AreaType::Lawn)),
        FeatureId::Paving => ToolKind::Rect(DrawTarget::Area(AreaType::Paving)),
        FeatureId::Gravel => ToolKind::Polygon(DrawTarget::Area(AreaType::Gravel)),
        FeatureId::Water => ToolKind::Ellipse(DrawTarget::Area(AreaType::Water)),
        FeatureId::Path => ToolKind::Polyline(LineTarget::Path { width: 900.0 }),
        FeatureId::Fence => line(LinearKind::Fence, 50.0, 1800.0),
        FeatureId::Wall => line(LinearKind::Wall, 200.0, 1800.0),
        FeatureId::Hedge => line(LinearKind::Hedge, 600.0, 1500.0),
        FeatureId::Shed => structure(StructureKind::Shed),
        FeatureId::Greenhouse => structure(StructureKind::Greenhouse),
        FeatureId::Pergola => structure(StructureKind::Pergola),
        FeatureId::RaisedBed => structure(StructureKind::RaisedBed),
        FeatureId::Seating => structure(StructureKind::Furniture),
    }
}

type Row = (
    &'static str,
    &'static str,
    C,
    f64,
    f64,
    S,
    &'static [&'static str],
);

/// (id, name, category, spread, height, symbol, other names)
const PLANTS: &[Row] = &[
    // trees
    (
        "oak",
        "Oak Tree",
        C::Tree,
        12000.,
        15000.,
        S::Round,
        &["english oak", "quercus"],
    ),
    (
        "birch",
        "Silver Birch",
        C::Tree,
        5000.,
        12000.,
        S::Leafy,
        &["betula"],
    ),
    (
        "apple",
        "Apple Tree",
        C::Tree,
        4000.,
        4000.,
        S::Round,
        &["fruit tree", "malus"],
    ),
    (
        "cherry",
        "Ornamental Cherry",
        C::Tree,
        5000.,
        6000.,
        S::Round,
        &["prunus", "flowering cherry", "blossom"],
    ),
    (
        "acer",
        "Japanese Maple",
        C::Tree,
        3000.,
        3000.,
        S::Leafy,
        &["acer", "acer palmatum"],
    ),
    (
        "crab_apple",
        "Crab Apple",
        C::Tree,
        4000.,
        5000.,
        S::Round,
        &["malus", "fruit tree"],
    ),
    (
        "rowan",
        "Rowan",
        C::Tree,
        4000.,
        8000.,
        S::Leafy,
        &["mountain ash", "sorbus"],
    ),
    (
        "magnolia",
        "Magnolia",
        C::Tree,
        4000.,
        5000.,
        S::Round,
        &["tulip tree"],
    ),
    (
        "olive",
        "Olive Tree",
        C::Tree,
        3000.,
        4000.,
        S::Leafy,
        &["olea"],
    ),
    ("holly", "Holly", C::Tree, 3000., 6000., S::Spiky, &["ilex"]),
    (
        "hawthorn",
        "Hawthorn",
        C::Tree,
        4000.,
        6000.,
        S::Round,
        &["may", "crataegus"],
    ),
    // shrubs
    (
        "yew",
        "Yew",
        C::Shrub,
        3000.,
        4000.,
        S::Conifer,
        &["taxus", "topiary"],
    ),
    (
        "box",
        "Box Ball",
        C::Shrub,
        600.,
        600.,
        S::Round,
        &["boxwood", "buxus", "topiary"],
    ),
    (
        "lavender",
        "Lavender",
        C::Shrub,
        600.,
        500.,
        S::Spiky,
        &["lavandula"],
    ),
    (
        "rose",
        "Shrub Rose",
        C::Shrub,
        1200.,
        1200.,
        S::Round,
        &["rosa", "bush rose"],
    ),
    (
        "hydrangea",
        "Hydrangea",
        C::Shrub,
        1500.,
        1500.,
        S::Round,
        &["mophead", "lacecap"],
    ),
    (
        "rosemary",
        "Rosemary",
        C::Shrub,
        900.,
        1000.,
        S::Spiky,
        &["salvia rosmarinus", "herb"],
    ),
    (
        "hazel",
        "Hazel",
        C::Shrub,
        4000.,
        5000.,
        S::Round,
        &["corylus", "cobnut"],
    ),
    (
        "buddleja",
        "Butterfly Bush",
        C::Shrub,
        2500.,
        3000.,
        S::Spiky,
        &["buddleja", "buddleia"],
    ),
    (
        "viburnum",
        "Viburnum",
        C::Shrub,
        2000.,
        2000.,
        S::Round,
        &["snowball bush"],
    ),
    (
        "pittosporum",
        "Pittosporum",
        C::Shrub,
        1500.,
        3000.,
        S::Round,
        &["kohuhu"],
    ),
    (
        "photinia",
        "Photinia",
        C::Shrub,
        2500.,
        3000.,
        S::Leafy,
        &["red robin"],
    ),
    (
        "hebe",
        "Hebe",
        C::Shrub,
        1000.,
        800.,
        S::Round,
        &["veronica"],
    ),
    ("skimmia", "Skimmia", C::Shrub, 1000., 1000., S::Round, &[]),
    (
        "dogwood",
        "Dogwood",
        C::Shrub,
        2000.,
        2000.,
        S::Leafy,
        &["cornus"],
    ),
    // perennials and annuals
    (
        "hosta",
        "Hosta",
        C::Perennial,
        800.,
        500.,
        S::Leafy,
        &["plantain lily"],
    ),
    (
        "salvia",
        "Salvia",
        C::Perennial,
        500.,
        600.,
        S::Spiky,
        &["sage", "meadow sage"],
    ),
    (
        "geranium",
        "Hardy Geranium",
        C::Perennial,
        600.,
        400.,
        S::Round,
        &["cranesbill"],
    ),
    (
        "fern",
        "Fern",
        C::Perennial,
        700.,
        600.,
        S::Leafy,
        &["dryopteris"],
    ),
    (
        "foxglove",
        "Foxglove",
        C::Perennial,
        500.,
        1500.,
        S::Spiky,
        &["digitalis"],
    ),
    (
        "echinacea",
        "Coneflower",
        C::Perennial,
        500.,
        900.,
        S::Star,
        &["echinacea"],
    ),
    (
        "peony",
        "Peony",
        C::Perennial,
        900.,
        900.,
        S::Round,
        &["paeonia"],
    ),
    (
        "allium",
        "Allium",
        C::Perennial,
        300.,
        1000.,
        S::Star,
        &["ornamental onion"],
    ),
    (
        "agapanthus",
        "Agapanthus",
        C::Perennial,
        600.,
        900.,
        S::Spiky,
        &["african lily"],
    ),
    (
        "sedum",
        "Sedum",
        C::Perennial,
        500.,
        500.,
        S::Round,
        &["stonecrop", "hylotelephium"],
    ),
    (
        "catmint",
        "Catmint",
        C::Perennial,
        600.,
        500.,
        S::Spiky,
        &["nepeta"],
    ),
    (
        "astrantia",
        "Astrantia",
        C::Perennial,
        500.,
        700.,
        S::Star,
        &["masterwort"],
    ),
    (
        "verbena",
        "Verbena",
        C::Perennial,
        500.,
        1500.,
        S::Star,
        &["verbena bonariensis", "purpletop"],
    ),
    (
        "rudbeckia",
        "Black-eyed Susan",
        C::Perennial,
        600.,
        900.,
        S::Star,
        &["rudbeckia"],
    ),
    (
        "penstemon",
        "Penstemon",
        C::Perennial,
        500.,
        800.,
        S::Spiky,
        &["beardtongue"],
    ),
    (
        "alchemilla",
        "Lady's Mantle",
        C::Perennial,
        600.,
        400.,
        S::Leafy,
        &["alchemilla"],
    ),
    (
        "heuchera",
        "Heuchera",
        C::Perennial,
        400.,
        400.,
        S::Leafy,
        &["coral bells"],
    ),
    (
        "cosmos",
        "Cosmos",
        C::Annual,
        600.,
        1200.,
        S::Star,
        &["annual", "bedding"],
    ),
    // grasses
    (
        "grass",
        "Ornamental Grass",
        C::Grass,
        800.,
        1200.,
        S::Spiky,
        &[],
    ),
    (
        "miscanthus",
        "Miscanthus",
        C::Grass,
        1500.,
        2000.,
        S::Spiky,
        &["silver grass", "maiden grass"],
    ),
    (
        "stipa",
        "Mexican Feather Grass",
        C::Grass,
        600.,
        800.,
        S::Spiky,
        &["stipa", "nassella"],
    ),
    (
        "pennisetum",
        "Fountain Grass",
        C::Grass,
        900.,
        900.,
        S::Spiky,
        &["pennisetum"],
    ),
    ("carex", "Sedge", C::Grass, 500., 400., S::Spiky, &["carex"]),
    (
        "fescue",
        "Blue Fescue",
        C::Grass,
        300.,
        300.,
        S::Spiky,
        &["festuca"],
    ),
    // climbers
    (
        "clematis",
        "Clematis",
        C::Climber,
        1000.,
        3000.,
        S::Star,
        &[],
    ),
    (
        "wisteria",
        "Wisteria",
        C::Climber,
        6000.,
        9000.,
        S::Leafy,
        &[],
    ),
    (
        "honeysuckle",
        "Honeysuckle",
        C::Climber,
        2000.,
        5000.,
        S::Star,
        &["lonicera"],
    ),
    (
        "climbing_rose",
        "Climbing Rose",
        C::Climber,
        3000.,
        4000.,
        S::Round,
        &["rambling rose", "rambler"],
    ),
    (
        "ivy",
        "Ivy",
        C::Climber,
        3000.,
        6000.,
        S::Leafy,
        &["hedera"],
    ),
    (
        "jasmine",
        "Star Jasmine",
        C::Climber,
        2500.,
        5000.,
        S::Star,
        &["trachelospermum", "jasmine"],
    ),
    // edibles
    ("tomato", "Tomato", C::Vegetable, 600., 1500., S::Leafy, &[]),
    (
        "lettuce",
        "Lettuce",
        C::Vegetable,
        300.,
        250.,
        S::Leafy,
        &["salad"],
    ),
    (
        "strawberry",
        "Strawberry",
        C::Vegetable,
        400.,
        200.,
        S::Leafy,
        &["fruit"],
    ),
    (
        "courgette",
        "Courgette",
        C::Vegetable,
        1200.,
        600.,
        S::Leafy,
        &["zucchini", "squash"],
    ),
    (
        "runner_bean",
        "Runner Beans",
        C::Vegetable,
        600.,
        2500.,
        S::Leafy,
        &["beans", "climbing beans"],
    ),
    (
        "kale",
        "Kale",
        C::Vegetable,
        600.,
        700.,
        S::Leafy,
        &["cabbage", "brassica"],
    ),
    ("potato", "Potato", C::Vegetable, 600., 600., S::Leafy, &[]),
    ("basil", "Basil", C::Herb, 300., 400., S::Leafy, &[]),
    ("mint", "Mint", C::Herb, 900., 600., S::Leafy, &["mentha"]),
    ("thyme", "Thyme", C::Herb, 400., 250., S::Spiky, &["thymus"]),
    ("chives", "Chives", C::Herb, 300., 400., S::Spiky, &[]),
    ("parsley", "Parsley", C::Herb, 400., 400., S::Leafy, &[]),
    (
        "sage_herb",
        "Common Sage",
        C::Herb,
        600.,
        600.,
        S::Leafy,
        &["sage", "salvia officinalis"],
    ),
];

fn features() -> Vec<(
    FeatureId,
    &'static str,
    &'static str,
    &'static [&'static str],
)> {
    use FeatureId::*;
    vec![
        (
            Bed,
            "bed",
            EN.feat_bed,
            &["border", "flower bed", "planting"],
        ),
        (Lawn, "lawn", EN.feat_lawn, &["grass", "turf"]),
        (
            Paving,
            "paving",
            EN.feat_paving,
            &["patio", "terrace", "slabs"],
        ),
        (Gravel, "gravel", EN.feat_gravel, &["shingle", "stones"]),
        (Water, "water", EN.feat_water, &["pond", "pool"]),
        (Path, "path", EN.feat_path, &["walkway", "stepping stones"]),
        (Fence, "fence", EN.feat_fence, &["panel", "boundary"]),
        (Wall, "wall", EN.feat_wall, &["brick", "retaining wall"]),
        (Hedge, "hedge", EN.feat_hedge, &["hedging"]),
        (Shed, "shed", EN.feat_shed, &["storage", "summerhouse"]),
        (
            Greenhouse,
            "greenhouse",
            EN.feat_greenhouse,
            &["glasshouse", "polytunnel"],
        ),
        (
            Pergola,
            "pergola",
            EN.feat_pergola,
            &["arbour", "arbor", "arch"],
        ),
        (
            RaisedBed,
            "raised_bed",
            EN.feat_raised_bed,
            &["planter", "veg bed"],
        ),
        (
            Seating,
            "seating",
            EN.feat_seating,
            &["bench", "chairs", "table", "furniture"],
        ),
    ]
}

/// Everything the catalog offers: plants grouped Trees…Edibles, then features.
pub fn catalog() -> Vec<CatalogEntry> {
    let mut entries: Vec<CatalogEntry> = PLANTS
        .iter()
        .map(
            |&(id, name, category, spread, height, symbol, aliases)| CatalogEntry {
                id: id.into(),
                name: name.into(),
                aliases: aliases.to_vec(),
                group: group_of(category),
                kind: EntryKind::Plant(PlantDef {
                    id: id.into(),
                    name: name.into(),
                    category,
                    spread,
                    height,
                    symbol,
                    custom: false,
                    aliases: Vec::new(),
                }),
            },
        )
        .collect();
    entries.sort_by_key(|e| e.group); // stable: keeps the order within a group
    entries.extend(
        features()
            .into_iter()
            .map(|(f, id, name, aliases)| CatalogEntry {
                id: format!("feature-{id}"),
                name: name.into(),
                aliases: aliases.to_vec(),
                group: Group::Features,
                kind: EntryKind::Feature(f),
            }),
    );
    entries
}

/// Every built-in plant, in catalog order.
pub fn builtin_plants() -> Vec<PlantDef> {
    catalog()
        .into_iter()
        .filter_map(|e| match e.kind {
            EntryKind::Plant(p) => Some(p),
            EntryKind::Feature(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{DrawTarget, LineTarget, ToolKind};
    use garden_core::model::{AreaType, LinearKind, StructureKind};

    const OLD_IDS: [&str; 20] = [
        "oak",
        "birch",
        "apple",
        "cherry",
        "acer",
        "yew",
        "box",
        "lavender",
        "rose",
        "hydrangea",
        "rosemary",
        "hosta",
        "salvia",
        "geranium",
        "grass",
        "fern",
        "clematis",
        "tomato",
        "lettuce",
        "basil",
    ];

    #[test]
    fn ids_are_unique_and_old_ids_unchanged() {
        let c = catalog();
        let mut ids: Vec<_> = c.iter().map(|e| e.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.len());
        let lav = builtin_plants()
            .into_iter()
            .find(|p| p.id == "lavender")
            .unwrap();
        assert_eq!(
            (lav.name.as_str(), lav.spread, lav.height),
            ("Lavender", 600., 500.)
        );
        for id in OLD_IDS {
            assert!(builtin_plants().iter().any(|p| p.id == id), "{id} missing");
        }
    }

    #[test]
    fn the_catalog_is_big_and_sane() {
        let plants = builtin_plants();
        assert!(plants.len() >= 55, "{}", plants.len());
        for p in &plants {
            assert!(
                p.spread >= 100. && p.spread <= 20_000. && p.height >= 50. && p.height <= 30_000.,
                "{}",
                p.id
            );
            assert!(!p.custom && !p.name.is_empty());
        }
        for g in [
            Group::Trees,
            Group::Shrubs,
            Group::Perennials,
            Group::Grasses,
            Group::Climbers,
            Group::Edibles,
        ] {
            assert!(
                catalog().iter().filter(|e| e.group == g).count() >= 4,
                "{g:?}"
            );
        }
        assert_eq!(
            catalog()
                .iter()
                .filter(|e| e.group == Group::Features)
                .count(),
            14
        );
        // plants come before features, grouped
        let groups: Vec<Group> = catalog().iter().map(|e| e.group).collect();
        let mut sorted = groups.clone();
        sorted.sort_by_key(|g| *g as u8);
        assert_eq!(groups, sorted);
    }

    #[test]
    fn features_pick_the_right_tool() {
        assert!(matches!(
            feature_tool(FeatureId::Greenhouse),
            ToolKind::Rect(DrawTarget::Structure(StructureKind::Greenhouse))
        ));
        assert!(matches!(
            feature_tool(FeatureId::Water),
            ToolKind::Ellipse(DrawTarget::Area(AreaType::Water))
        ));
        assert!(matches!(
            feature_tool(FeatureId::Hedge),
            ToolKind::Polyline(LineTarget::Linear {
                kind: LinearKind::Hedge,
                ..
            })
        ));
        assert_eq!(structure_height(StructureKind::RaisedBed), 400.);
        assert_eq!(structure_height(StructureKind::Shed), 2200.);
    }
}
