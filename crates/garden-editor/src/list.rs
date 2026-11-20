//! The garden list: what the design needs (plants, areas, lengths,
//! structures), with optional prices, a total and a CSV export.

use std::collections::BTreeMap;

use garden_core::model::{
    AreaType, Document, LinearKind, ObjectKind, PlantCategory, StructureKind,
};
use garden_core::units::{UnitSystem, format_area, format_length};

use crate::catalog::{Group, group_of};
use crate::strings::EN;

/// What a row is counted in. Quantities are always metric.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Each,
    Metre,
    SquareMetre,
    CubicMetre,
}

/// Where a row is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Plants(Group),
    Areas,
    Lengths,
    Structures,
}

/// One line of the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Stable key prices are saved under, e.g. "plant:apple", "area:lawn".
    pub key: String,
    pub section: Section,
    pub label: String,
    /// In `unit`, metric (count, m, m², m³).
    pub quantity: f64,
    pub unit: Unit,
    /// Extra information, e.g. a plant's full-grown size; may be empty.
    pub detail: String,
    /// False for rows that are only information (the plot).
    pub priceable: bool,
}

/// Everything in `doc` worth listing; gravel as a volume at `gravel_depth_mm`.
pub fn garden_list(doc: &Document, gravel_depth_mm: f64) -> Vec<Row> {
    let depth = if gravel_depth_mm.is_finite() && gravel_depth_mm > 0.0 {
        gravel_depth_mm
    } else {
        0.0
    };
    let units = doc.units;
    let mut plants: Vec<Row> = Vec::new();
    // areas (mm²) by type, gravel (mm²), paths (mm², mm), lengths (mm), plot (mm²), structures
    let mut areas: BTreeMap<u8, f64> = BTreeMap::new();
    let mut gravel = 0.0;
    let (mut path_area, mut path_length) = (0.0, 0.0);
    let mut lengths: BTreeMap<u8, f64> = BTreeMap::new();
    let mut plot = 0.0;
    let mut structures: BTreeMap<u8, f64> = BTreeMap::new();
    let pos = |x: f64| if x.is_finite() && x > 0.0 { x } else { 0.0 };
    for o in &doc.objects {
        match &o.kind {
            ObjectKind::Plant {
                spread,
                height,
                category,
                catalog_id,
                ..
            } => {
                let key = match catalog_id {
                    Some(id) => format!("plant:{id}"),
                    None => format!("plant:custom:{}:{}", category_key(*category), o.name),
                };
                match plants.iter_mut().find(|r| r.key == key) {
                    Some(r) => r.quantity += 1.0,
                    None => plants.push(Row {
                        key,
                        section: Section::Plants(group_of(*category)),
                        label: o.name.clone(),
                        quantity: 1.0,
                        unit: Unit::Each,
                        detail: format!(
                            "{} × {}",
                            format_length(*spread, units),
                            format_length(*height, units)
                        ),
                        priceable: true,
                    }),
                }
            }
            ObjectKind::Area { area_type, shape } => {
                let a = pos(shape.filled_area());
                if *area_type == AreaType::Gravel {
                    gravel += a;
                } else {
                    *areas.entry(area_order(*area_type)).or_default() += a;
                }
            }
            ObjectKind::Path { shape, width } => {
                let l = pos(shape.length());
                path_length += l;
                path_area += l * pos(*width);
            }
            ObjectKind::Linear { kind, shape, .. } => {
                *lengths.entry(linear_order(*kind)).or_default() += pos(shape.length());
            }
            ObjectKind::PlotBoundary { shape } => plot += pos(shape.filled_area()),
            ObjectKind::Structure { kind, .. } => {
                *structures.entry(structure_order(*kind)).or_default() += 1.0;
            }
            ObjectKind::Label { .. } | ObjectKind::Dimension { .. } => {}
        }
    }
    plants.sort_by(|a, b| {
        a.section
            .cmp(&b.section)
            .then_with(|| a.label.cmp(&b.label))
    });
    let mut rows = plants;
    let mut push = |key: String, section, label: &str, quantity: f64, unit, detail: String| {
        if quantity > 0.0 {
            rows.push(Row {
                priceable: key != "plot",
                key,
                section,
                label: label.to_string(),
                quantity,
                unit,
                detail,
            });
        }
    };
    for (&i, &mm2) in &areas {
        let t = AREA_TYPES[i as usize];
        push(
            format!("area:{}", area_key(t)),
            Section::Areas,
            area_label(t),
            mm2 / 1e6,
            Unit::SquareMetre,
            String::new(),
        );
    }
    push(
        "gravel".into(),
        Section::Areas,
        EN.kind_gravel,
        gravel * depth / 1e9,
        Unit::CubicMetre,
        format!(
            "{} · {} {}",
            format_area(gravel, units),
            format_length(depth, units),
            EN.list_deep
        ),
    );
    push(
        "path".into(),
        Section::Areas,
        EN.kind_path,
        path_area / 1e6,
        Unit::SquareMetre,
        format!("{} {}", format_length(path_length, units), EN.list_long),
    );
    push(
        "plot".into(),
        Section::Areas,
        EN.kind_plot,
        plot / 1e6,
        Unit::SquareMetre,
        String::new(),
    );
    for (&i, &mm) in &lengths {
        let k = LINEAR_KINDS[i as usize];
        push(
            format!("linear:{}", linear_key(k)),
            Section::Lengths,
            linear_label(k),
            mm / 1000.0,
            Unit::Metre,
            String::new(),
        );
    }
    for (&i, &n) in &structures {
        let k = STRUCTURE_KINDS[i as usize];
        push(
            format!("structure:{}", structure_key(k)),
            Section::Structures,
            structure_label(k),
            n,
            Unit::Each,
            String::new(),
        );
    }
    rows
}

const SQ_FT_PER_M2: f64 = 1.0 / 0.092_903_04;
const FT_PER_M: f64 = 1.0 / 0.3048;
const CU_YD_PER_M3: f64 = 1.0 / 0.764_554_857_984;

/// How many shown units make one metric unit.
fn shown_per_metric(unit: Unit, system: UnitSystem) -> f64 {
    match (system, unit) {
        (UnitSystem::Metric, _) | (_, Unit::Each) => 1.0,
        (UnitSystem::Imperial, Unit::Metre) => FT_PER_M,
        (UnitSystem::Imperial, Unit::SquareMetre) => SQ_FT_PER_M2,
        (UnitSystem::Imperial, Unit::CubicMetre) => CU_YD_PER_M3,
    }
}

/// A price per metric unit, as a price per the unit shown in `system`.
pub fn price_to_display(per_metric: f64, unit: Unit, system: UnitSystem) -> f64 {
    per_metric / shown_per_metric(unit, system)
}

/// A price per the unit shown in `system`, as a price per metric unit.
pub fn price_from_display(per_shown: f64, unit: Unit, system: UnitSystem) -> f64 {
    per_shown * shown_per_metric(unit, system)
}

/// The unit people read, e.g. "m²" or "ft²"; empty for counts.
pub fn unit_label(unit: Unit, system: UnitSystem) -> &'static str {
    match (system, unit) {
        (_, Unit::Each) => "",
        (UnitSystem::Metric, Unit::Metre) => "m",
        (UnitSystem::Metric, Unit::SquareMetre) => "m²",
        (UnitSystem::Metric, Unit::CubicMetre) => "m³",
        (UnitSystem::Imperial, Unit::Metre) => "ft",
        (UnitSystem::Imperial, Unit::SquareMetre) => "ft²",
        (UnitSystem::Imperial, Unit::CubicMetre) => "yd³",
    }
}

/// The row's quantity in the units people read, e.g. "12", "45.5 m²", "490 ft²".
pub fn quantity_display(row: &Row, system: UnitSystem) -> String {
    match row.unit {
        Unit::Each => format!("{:.0}", row.quantity),
        Unit::Metre => format_length(row.quantity * 1000.0, system),
        Unit::SquareMetre => format_area(row.quantity * 1e6, system),
        Unit::CubicMetre => format!(
            "{:.2} {}",
            row.quantity * shown_per_metric(Unit::CubicMetre, system),
            unit_label(Unit::CubicMetre, system)
        ),
    }
}

/// The row's cost, if it has a price.
pub fn cost(row: &Row, prices: &BTreeMap<String, f64>) -> Option<f64> {
    if !row.priceable {
        return None;
    }
    prices
        .get(&row.key)
        .filter(|p| p.is_finite())
        .map(|p| p * row.quantity)
}

/// The sum of all costs; None when no row has a price.
pub fn total(rows: &[Row], prices: &BTreeMap<String, f64>) -> Option<f64> {
    rows.iter()
        .filter_map(|r| cost(r, prices))
        .reduce(|a, b| a + b)
}

/// Section heading people read.
pub fn section_label(section: Section) -> &'static str {
    match section {
        Section::Plants(g) => g.label(),
        Section::Areas => EN.list_areas,
        Section::Lengths => EN.list_lengths,
        Section::Structures => EN.list_structures,
    }
}

/// One CSV cell: quoted when needed, and never read as a formula.
fn csv_cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_string()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// The list as CSV (RFC 4180), safe to open in a spreadsheet.
pub fn to_csv(
    rows: &[Row],
    prices: &BTreeMap<String, f64>,
    system: UnitSystem,
    currency: &str,
) -> String {
    let mut out = String::from("Section,Item,Quantity,Unit,Price,Cost\r\n");
    for r in rows {
        let factor = shown_per_metric(r.unit, system);
        let quantity = match r.unit {
            Unit::Each => format!("{:.0}", r.quantity),
            _ => format!("{:.2}", r.quantity * factor),
        };
        let price = prices
            .get(&r.key)
            .filter(|_| r.priceable)
            .map(|p| price_text(*p, r.unit, system))
            .unwrap_or_default();
        let cost = cost(r, prices)
            .map(|c| format!("{c:.2}"))
            .unwrap_or_default();
        let unit = match r.unit {
            Unit::Each => EN.list_each,
            u => unit_label(u, system),
        };
        let cells = [
            csv_cell(section_label(r.section)),
            csv_cell(&r.label),
            quantity,
            csv_cell(unit),
            price,
            cost,
        ];
        out.push_str(&cells.join(","));
        out.push_str("\r\n");
    }
    if let Some(t) = total(rows, prices) {
        let currency = csv_cell(currency.trim());
        out.push_str(&format!(
            "{},,,,{currency},{t:.2}\r\n",
            csv_cell(EN.list_total)
        ));
    }
    out
}

fn category_key(c: PlantCategory) -> &'static str {
    match c {
        PlantCategory::Tree => "tree",
        PlantCategory::FruitTree => "fruit_tree",
        PlantCategory::Shrub => "shrub",
        PlantCategory::Perennial => "perennial",
        PlantCategory::Annual => "annual",
        PlantCategory::Grass => "grass",
        PlantCategory::Climber => "climber",
        PlantCategory::Vegetable => "vegetable",
        PlantCategory::Herb => "herb",
        PlantCategory::Other => "other",
    }
}

const AREA_TYPES: [AreaType; 5] = [
    AreaType::Bed,
    AreaType::Lawn,
    AreaType::Paving,
    AreaType::Water,
    AreaType::Other,
];

fn area_order(t: AreaType) -> u8 {
    AREA_TYPES.iter().position(|&x| x == t).unwrap_or(4) as u8
}

fn area_key(t: AreaType) -> &'static str {
    match t {
        AreaType::Bed => "bed",
        AreaType::Lawn => "lawn",
        AreaType::Paving => "paving",
        AreaType::Gravel => "gravel",
        AreaType::Water => "water",
        AreaType::Other => "other",
    }
}

fn area_label(t: AreaType) -> &'static str {
    match t {
        AreaType::Bed => EN.kind_bed,
        AreaType::Lawn => EN.kind_lawn,
        AreaType::Paving => EN.kind_paving,
        AreaType::Gravel => EN.kind_gravel,
        AreaType::Water => EN.kind_water,
        AreaType::Other => EN.kind_area,
    }
}

const LINEAR_KINDS: [LinearKind; 3] = [LinearKind::Fence, LinearKind::Wall, LinearKind::Hedge];

fn linear_order(k: LinearKind) -> u8 {
    LINEAR_KINDS.iter().position(|&x| x == k).unwrap_or(0) as u8
}

fn linear_key(k: LinearKind) -> &'static str {
    match k {
        LinearKind::Fence => "fence",
        LinearKind::Wall => "wall",
        LinearKind::Hedge => "hedge",
    }
}

fn linear_label(k: LinearKind) -> &'static str {
    match k {
        LinearKind::Fence => EN.kind_fence,
        LinearKind::Wall => EN.kind_wall,
        LinearKind::Hedge => EN.kind_hedge,
    }
}

const STRUCTURE_KINDS: [StructureKind; 6] = [
    StructureKind::Shed,
    StructureKind::Greenhouse,
    StructureKind::Pergola,
    StructureKind::RaisedBed,
    StructureKind::Furniture,
    StructureKind::Other,
];

fn structure_order(k: StructureKind) -> u8 {
    STRUCTURE_KINDS.iter().position(|&x| x == k).unwrap_or(5) as u8
}

fn structure_key(k: StructureKind) -> &'static str {
    match k {
        StructureKind::Shed => "shed",
        StructureKind::Greenhouse => "greenhouse",
        StructureKind::Pergola => "pergola",
        StructureKind::RaisedBed => "raised_bed",
        StructureKind::Furniture => "furniture",
        StructureKind::Other => "other",
    }
}

fn structure_label(k: StructureKind) -> &'static str {
    match k {
        StructureKind::Shed => EN.kind_shed,
        StructureKind::Greenhouse => EN.kind_greenhouse,
        StructureKind::Pergola => EN.kind_pergola,
        StructureKind::RaisedBed => EN.kind_raised_bed,
        StructureKind::Furniture => EN.kind_furniture,
        StructureKind::Other => EN.kind_structure,
    }
}

/// A price as the person typed it: blank clears it. A comma is the decimal
/// point ("4,50") unless the text also has a dot ("1,234.50"); "1,500" is
/// refused because it could mean either.
pub fn parse_price(text: &str) -> Result<Option<f64>, PriceError> {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if t.is_empty() {
        return Ok(None);
    }
    let normal = if t.contains('.') {
        t.replace(',', "")
    } else {
        let parts: Vec<&str> = t.split(',').collect();
        match parts.as_slice() {
            [_] => t.clone(),
            // one comma: the decimal point, unless exactly 3 digits follow
            [whole, frac] if frac.len() != 3 => format!("{whole}.{frac}"),
            _ => return Err(PriceError),
        }
    };
    if !normal
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    {
        return Err(PriceError);
    }
    match normal.parse::<f64>() {
        Ok(p) if (0.0..1e12).contains(&p) => Ok(Some(p + 0.0)),
        _ => Err(PriceError),
    }
}

/// A price couldn't be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriceError;

/// A price per metric unit as shown in `system`: up to 4 decimals, no trailing zeros.
pub fn price_text(per_metric: f64, unit: Unit, system: UnitSystem) -> String {
    let t = format!("{:.4}", price_to_display(per_metric, unit, system));
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A money amount with two decimals and the currency, e.g. "€ 12.50".
pub fn money(amount: f64, currency: &str) -> String {
    let currency = currency.trim();
    if currency.is_empty() {
        format!("{amount:.2}")
    } else {
        format!("{currency} {amount:.2}")
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// "per m²", "per ft²", "each".
pub fn per_unit(unit: Unit, system: UnitSystem) -> String {
    match unit {
        Unit::Each => EN.list_each.to_string(),
        u => format!("{} {}", EN.list_per, unit_label(u, system)),
    }
}

/// The list as a printable page (HTML, everything escaped).
pub fn print_html(
    name: &str,
    date: &str,
    rows: &[Row],
    prices: &BTreeMap<String, f64>,
    system: UnitSystem,
    currency: &str,
) -> String {
    let priced = total(rows, prices).is_some();
    let mut body = String::new();
    let mut section = None;
    for r in rows {
        if section != Some(r.section) {
            section = Some(r.section);
            body += &format!(
                r#"<tr class="list-section"><th colspan="{}">{}</th></tr>"#,
                if priced { 4 } else { 2 },
                escape(section_label(r.section))
            );
        }
        let detail = if r.detail.is_empty() {
            String::new()
        } else {
            format!("<small>{}</small>", escape(&r.detail))
        };
        let money_cells = if priced {
            let price = prices
                .get(&r.key)
                .filter(|_| r.priceable)
                .map(|p| {
                    let shown = price_text(*p, r.unit, system);
                    let shown = if currency.trim().is_empty() {
                        shown
                    } else {
                        format!("{} {shown}", currency.trim())
                    };
                    escape(&format!("{shown} {}", per_unit(r.unit, system)))
                })
                .unwrap_or_default();
            let c = cost(r, prices)
                .map(|c| escape(&money(c, currency)))
                .unwrap_or_default();
            format!(r#"<td>{price}</td><td class="num">{c}</td>"#)
        } else {
            String::new()
        };
        body += &format!(
            r#"<tr><td>{}{detail}</td><td class="num">{}</td>{money_cells}</tr>"#,
            escape(&r.label),
            escape(&quantity_display(r, system)),
        );
    }
    let foot = total(rows, prices)
        .map(|t| {
            format!(
                r#"<tfoot><tr><th colspan="3">{}</th><th class="num">{}</th></tr></tfoot>"#,
                escape(EN.list_total),
                escape(&money(t, currency))
            )
        })
        .unwrap_or_default();
    format!(
        r#"<section class="print-page print-list"><header class="print-title"><strong>{}</strong><span>{}</span><span>{}</span></header><table>{body}{foot}</table></section>"#,
        escape(name),
        escape(EN.list_title),
        escape(date),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use garden_core::command::Command;
    use garden_core::geometry::{Point, Shape};
    use garden_core::model::{
        AreaType, Layer, LinearKind, Object, ObjectKind, PlantCategory, PlantSymbol, StructureKind,
    };

    fn add(doc: &mut Document, name: &str, kind: ObjectKind) {
        let layer = doc.layers[0].id;
        let index = doc.objects.len();
        doc.apply(
            Command::Insert {
                object: Object::new(name, kind, layer),
                index,
            },
            1,
        )
        .unwrap();
    }

    fn plant(catalog_id: Option<&str>, category: PlantCategory) -> ObjectKind {
        ObjectKind::Plant {
            position: Point::new(0., 0.),
            spread: 4000.,
            height: 4000.,
            symbol: PlantSymbol::Round,
            category,
            catalog_id: catalog_id.map(Into::into),
        }
    }

    fn rect(w: f64, h: f64) -> Shape {
        Shape::rect(Point::new(0., 0.), w, h)
    }

    fn row<'a>(rows: &'a [Row], key: &str) -> &'a Row {
        rows.iter().find(|r| r.key == key).unwrap_or_else(|| {
            panic!(
                "no row {key}: {:?}",
                rows.iter().map(|r| &r.key).collect::<Vec<_>>()
            )
        })
    }

    #[test]
    fn counts_plants_by_kind() {
        let mut d = Document::new("g", 0);
        for _ in 0..3 {
            add(
                &mut d,
                "Apple Tree",
                plant(Some("apple"), PlantCategory::FruitTree),
            );
        }
        add(&mut d, "My Fig", plant(None, PlantCategory::FruitTree));
        // a second layer, hidden, still counts
        let hidden = Layer {
            id: garden_core::model::LayerId::new(),
            name: "hidden".into(),
            visible: false,
            locked: false,
        };
        let hidden_id = hidden.id;
        d.apply(Command::UpsertLayer { layer: hidden }, 1).unwrap();
        let o = Object::new("My Fig", plant(None, PlantCategory::FruitTree), hidden_id);
        d.apply(
            Command::Insert {
                object: o,
                index: 4,
            },
            1,
        )
        .unwrap();
        let rows = garden_list(&d, 50.);
        let apple = row(&rows, "plant:apple");
        assert_eq!((apple.quantity, apple.unit), (3., Unit::Each));
        assert_eq!(apple.section, Section::Plants(Group::FruitTrees));
        assert_eq!(apple.label, "Apple Tree");
        assert!(!apple.detail.is_empty());
        assert_eq!(row(&rows, "plant:custom:fruit_tree:My Fig").quantity, 2.);
    }

    #[test]
    fn areas_lengths_and_volume() {
        let mut d = Document::new("g", 0);
        add(
            &mut d,
            "lawn",
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: rect(10_000., 5000.),
            },
        );
        let reversed = Shape::polyline(
            &[
                Point::new(0., 0.),
                Point::new(0., 5000.),
                Point::new(10_000., 5000.),
                Point::new(10_000., 0.),
            ],
            true,
        );
        add(
            &mut d,
            "lawn 2",
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: reversed,
            },
        );
        add(
            &mut d,
            "gravel",
            ObjectKind::Area {
                area_type: AreaType::Gravel,
                shape: rect(4000., 2000.),
            },
        );
        add(
            &mut d,
            "fence",
            ObjectKind::Linear {
                kind: LinearKind::Fence,
                shape: Shape::polyline(&[Point::new(0., 0.), Point::new(12_000., 0.)], false),
                thickness: 50.,
                height: 1800.,
            },
        );
        add(
            &mut d,
            "path",
            ObjectKind::Path {
                shape: Shape::polyline(&[Point::new(0., 0.), Point::new(10_000., 0.)], false),
                width: 1000.,
            },
        );
        add(
            &mut d,
            "plot",
            ObjectKind::PlotBoundary {
                shape: rect(20_000., 10_000.),
            },
        );
        for _ in 0..2 {
            add(
                &mut d,
                "shed",
                ObjectKind::Structure {
                    kind: StructureKind::Shed,
                    footprint: rect(2000., 2000.),
                    height: 2200.,
                },
            );
        }
        let rows = garden_list(&d, 50.);
        let close = |key: &str, want: f64, unit: Unit| {
            let r = row(&rows, key);
            assert!((r.quantity - want).abs() < 1e-6, "{key}: {}", r.quantity);
            assert_eq!(r.unit, unit, "{key}");
        };
        close("area:lawn", 100., Unit::SquareMetre); // both lawns, 50 m² each
        close("gravel", 0.4, Unit::CubicMetre);
        close("linear:fence", 12., Unit::Metre);
        close("path", 10., Unit::SquareMetre);
        close("plot", 200., Unit::SquareMetre);
        close("structure:shed", 2., Unit::Each);
        assert!(!row(&rows, "plot").priceable);
        assert!(row(&rows, "area:lawn").priceable);
        assert!(
            rows.iter().all(|r| r.key != "area:gravel"),
            "gravel is listed by volume only"
        );
    }

    #[test]
    fn no_nan_from_odd_shapes() {
        let mut d = Document::new("g", 0);
        let bow_tie = Shape::polyline(
            &[
                Point::new(0., 0.),
                Point::new(1000., 1000.),
                Point::new(1000., 0.),
                Point::new(0., 1000.),
            ],
            true,
        );
        add(
            &mut d,
            "a",
            ObjectKind::Area {
                area_type: AreaType::Bed,
                shape: bow_tie,
            },
        );
        let two = Shape::polyline(&[Point::new(0., 0.), Point::new(1000., 0.)], true);
        add(
            &mut d,
            "b",
            ObjectKind::Area {
                area_type: AreaType::Paving,
                shape: two,
            },
        );
        for r in garden_list(&d, 50.) {
            assert!(
                r.quantity.is_finite() && r.quantity >= 0.0,
                "{}: {}",
                r.key,
                r.quantity
            );
        }
        // the figure-eight bed counts both loops: 2 × 0.25 m²
        let rows = garden_list(&d, 50.);
        assert!((row(&rows, "area:bed").quantity - 0.5).abs() < 1e-6);
        assert!(
            garden_list(&d, f64::NAN)
                .iter()
                .all(|r| r.quantity.is_finite())
        );
    }

    #[test]
    fn prices_survive_unit_switching() {
        let shown = price_to_display(10., Unit::SquareMetre, UnitSystem::Imperial);
        assert!((shown - 0.929_030_4).abs() < 1e-6, "{shown}");
        for unit in [Unit::Each, Unit::Metre, Unit::SquareMetre, Unit::CubicMetre] {
            for system in [UnitSystem::Metric, UnitSystem::Imperial] {
                let back = price_from_display(price_to_display(7.25, unit, system), unit, system);
                assert!((back - 7.25).abs() < 1e-9, "{unit:?} {system:?}");
            }
        }
        // a lawn of 50 m² at 10 per m²: 500 whichever units the price was typed in
        let lawn = Row {
            key: "area:lawn".into(),
            section: Section::Areas,
            label: "Lawn".into(),
            quantity: 50.,
            unit: Unit::SquareMetre,
            detail: String::new(),
            priceable: true,
        };
        let typed_in_feet = price_from_display(shown, Unit::SquareMetre, UnitSystem::Imperial);
        let prices = BTreeMap::from([("area:lawn".to_string(), typed_in_feet)]);
        assert!((cost(&lawn, &prices).unwrap() - 500.).abs() < 1e-6);
        assert_eq!(quantity_display(&lawn, UnitSystem::Metric), "50.0 m²");
        assert_eq!(quantity_display(&lawn, UnitSystem::Imperial), "538 ft²");
    }

    #[test]
    fn totals() {
        let mut d = Document::new("g", 0);
        add(
            &mut d,
            "lawn",
            ObjectKind::Area {
                area_type: AreaType::Lawn,
                shape: rect(10_000., 5000.),
            },
        );
        for _ in 0..4 {
            add(
                &mut d,
                "Apple Tree",
                plant(Some("apple"), PlantCategory::FruitTree),
            );
        }
        let rows = garden_list(&d, 50.);
        assert_eq!(total(&rows, &BTreeMap::new()), None);
        let prices = BTreeMap::from([
            ("area:lawn".to_string(), 2.),
            ("plant:apple".to_string(), 30.),
            ("plant:gone".to_string(), 99.), // no such row: ignored
        ]);
        assert!((total(&rows, &prices).unwrap() - 220.).abs() < 1e-9);
    }

    #[test]
    fn csv_is_safe() {
        let mut d = Document::new("g", 0);
        add(&mut d, "=SUM(A1), \"x\"", plant(None, PlantCategory::Shrub));
        add(&mut d, "two\nlines", plant(None, PlantCategory::Shrub));
        let rows = garden_list(&d, 50.);
        let csv = to_csv(&rows, &BTreeMap::new(), UnitSystem::Metric, "€");
        assert!(csv.contains("\"'=SUM(A1), \"\"x\"\"\""), "{csv}");
        assert!(csv.contains("\"two\nlines\""), "{csv}");
        assert!(!csv.to_lowercase().contains("total"), "{csv}");
        let key = rows[0].key.clone();
        let priced = to_csv(&rows, &BTreeMap::from([(key, 5.)]), UnitSystem::Metric, "€");
        assert!(priced.lines().last().unwrap().contains("5.00"), "{priced}");
        assert!(
            priced.starts_with("Section,Item,Quantity,Unit,Price,Cost"),
            "{priced}"
        );
    }

    #[test]
    fn printed_list_escapes_everything() {
        let mut d = Document::new("<b>Mine</b>", 0);
        add(
            &mut d,
            "<script>x</script>",
            plant(None, PlantCategory::Shrub),
        );
        let rows = garden_list(&d, 50.);
        let prices = BTreeMap::from([(rows[0].key.clone(), 2.0)]);
        let html = print_html(
            &d.name,
            "1/1/2026",
            &rows,
            &prices,
            UnitSystem::Metric,
            "<img src=x onerror=alert(1)>",
        );
        assert!(
            !html.contains("<img") && !html.contains("<script") && !html.contains("<b>"),
            "{html}"
        );
        assert!(
            html.contains("&lt;img src=x onerror=alert(1)&gt; 2.00"),
            "{html}"
        );
        assert!(html.contains("&lt;b&gt;Mine&lt;/b&gt;"), "{html}");
    }

    #[test]
    fn prices_read_as_people_type_them() {
        assert_eq!(parse_price(""), Ok(None));
        assert_eq!(parse_price("  "), Ok(None));
        assert_eq!(parse_price("12.50"), Ok(Some(12.5)));
        assert_eq!(parse_price("4,50"), Ok(Some(4.5)));
        assert_eq!(parse_price("4,5"), Ok(Some(4.5)));
        assert_eq!(parse_price("1,234.50"), Ok(Some(1234.5)));
        assert_eq!(parse_price("1 500"), Ok(Some(1500.0)));
        assert_eq!(parse_price("1500"), Ok(Some(1500.0)));
        assert_eq!(
            parse_price("1,500"),
            Err(PriceError),
            "could be 1.5 or 1500"
        );
        assert_eq!(parse_price("-0"), Ok(Some(0.0)));
        assert!(parse_price("-0").unwrap().unwrap().is_sign_positive());
        for bad in ["abc", "-3", "1e400", "NaN", "inf", "1.2.3"] {
            assert_eq!(parse_price(bad), Err(PriceError), "{bad}");
        }
    }

    #[test]
    fn shown_prices_multiply_out() {
        // 4.5 per m² is 0.41806… per ft²: shown with enough decimals everywhere
        assert_eq!(
            price_text(4.5, Unit::SquareMetre, UnitSystem::Imperial),
            "0.4181"
        );
        assert_eq!(
            price_text(4.5, Unit::SquareMetre, UnitSystem::Metric),
            "4.5"
        );
        assert_eq!(price_text(6.0, Unit::Each, UnitSystem::Imperial), "6");
        assert_eq!(money(12.5, ""), "12.50");
        assert_eq!(money(12.5, "€"), "€ 12.50");
        let lawn = Row {
            key: "area:lawn".into(),
            section: Section::Areas,
            label: "Lawn".into(),
            quantity: 50.,
            unit: Unit::SquareMetre,
            detail: String::new(),
            priceable: true,
        };
        let prices = BTreeMap::from([("area:lawn".to_string(), 4.5)]);
        let csv = to_csv(
            std::slice::from_ref(&lawn),
            &prices,
            UnitSystem::Imperial,
            "",
        );
        assert!(csv.contains(",0.4181,225.00"), "{csv}");
        let html = print_html("g", "d", &[lawn], &prices, UnitSystem::Imperial, "");
        assert!(html.contains("0.4181"), "{html}");
    }
}
