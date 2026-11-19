//! Turns a document into a flat list of things to draw (in world mm).

use garden_core::geometry::Point;
use garden_core::model::{
    AreaType, Color, Document, LinearKind, ObjectId, ObjectKind, PlantCategory, PlantSymbol,
};
use garden_core::query::dimension_line;
use garden_core::units::format_length;

#[derive(Clone, Debug, PartialEq)]
pub enum RenderShape {
    /// SVG path data in world mm.
    Path {
        d: String,
        closed: bool,
    },
    /// A plant canopy: drawn as a symbol of radius `r` (half the spread).
    Plant {
        center: Point,
        r: f64,
        symbol: PlantSymbol,
        category: PlantCategory,
    },
    Line {
        from: Point,
        to: Point,
    },
    /// `size` in mm; 0 means "default screen size".
    Text {
        at: Point,
        text: String,
        size: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderItem {
    pub id: ObjectId,
    pub shape: RenderShape,
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    /// World mm; `None` = thin screen-space hairline.
    pub stroke_width: Option<f64>,
    pub selected: bool,
    /// Style key for the renderer (e.g. "lawn", "bed", "fence", "plant").
    pub class: &'static str,
}

/// Style key for an object kind; see [`RenderItem::class`].
pub fn style_class(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::PlotBoundary { .. } => "plot",
        ObjectKind::Area { area_type, .. } => match area_type {
            AreaType::Bed => "bed",
            AreaType::Lawn => "lawn",
            AreaType::Paving => "paving",
            AreaType::Gravel => "gravel",
            AreaType::Water => "water",
            AreaType::Other => "area",
        },
        ObjectKind::Path { .. } => "path",
        ObjectKind::Linear { kind, .. } => match kind {
            LinearKind::Fence => "fence",
            LinearKind::Wall => "wall",
            LinearKind::Hedge => "hedge",
        },
        ObjectKind::Structure { .. } => "structure",
        ObjectKind::Plant { .. } => "plant",
        ObjectKind::Label { .. } => "label",
        ObjectKind::Dimension { .. } => "dimension",
    }
}

const INK: Color = Color(43, 58, 48, 255);

/// Default (fill, stroke) colours for each kind of object.
pub fn default_style(kind: &ObjectKind) -> (Option<Color>, Option<Color>) {
    match kind {
        ObjectKind::PlotBoundary { .. } => (None, Some(INK)),
        ObjectKind::Area { area_type, .. } => {
            let (fill, edge) = match area_type {
                AreaType::Lawn => (Color(169, 203, 126, 255), Color(111, 154, 72, 255)),
                AreaType::Bed => (Color(156, 116, 88, 255), Color(110, 78, 56, 255)),
                AreaType::Paving => (Color(217, 211, 199, 255), Color(160, 152, 136, 255)),
                AreaType::Gravel => (Color(230, 223, 207, 255), Color(168, 157, 132, 255)),
                AreaType::Water => (Color(134, 190, 219, 255), Color(91, 147, 181, 255)),
                AreaType::Other => (Color(210, 210, 196, 255), Color(150, 150, 136, 255)),
            };
            (Some(fill), Some(edge))
        }
        ObjectKind::Structure { .. } => (
            Some(Color(196, 154, 108, 255)),
            Some(Color(110, 78, 51, 255)),
        ),
        ObjectKind::Path { .. } => (None, Some(Color(230, 217, 188, 255))),
        ObjectKind::Linear { kind, .. } => {
            let c = match kind {
                LinearKind::Fence => Color(138, 100, 64, 255),
                LinearKind::Wall => Color(169, 163, 151, 255),
                LinearKind::Hedge => Color(78, 122, 69, 255),
            };
            (None, Some(c))
        }
        ObjectKind::Plant { category, .. } => {
            // (fill, outline): a botanical-plate palette, one hue family per kind of plant
            let (fill, edge) = match category {
                PlantCategory::Tree => (Color(95, 145, 80, 140), Color(47, 82, 48, 255)),
                PlantCategory::Shrub => (Color(127, 174, 94, 220), Color(60, 102, 48, 255)),
                PlantCategory::Perennial => (Color(180, 154, 208, 225), Color(110, 85, 150, 255)),
                PlantCategory::Annual => (Color(238, 142, 122, 225), Color(176, 80, 63, 255)),
                PlantCategory::Grass => (Color(210, 195, 118, 225), Color(142, 127, 53, 255)),
                PlantCategory::Climber => (Color(157, 184, 106, 225), Color(91, 122, 51, 255)),
                PlantCategory::Vegetable => (Color(140, 192, 99, 225), Color(74, 122, 46, 255)),
                PlantCategory::Herb => (Color(167, 185, 160, 225), Color(94, 113, 88, 255)),
                PlantCategory::Other => (Color(158, 187, 138, 225), Color(84, 110, 70, 255)),
            };
            (Some(fill), Some(edge))
        }
        ObjectKind::Label { .. } | ObjectKind::Dimension { .. } => (None, Some(INK)),
    }
}

pub fn color_css(c: Color) -> String {
    format!(
        "rgba({},{},{},{:.3})",
        c.0,
        c.1,
        c.2,
        f64::from(c.3) / 255.0
    )
}

/// Outline of an object as drawn in move previews.
pub fn outline(kind: &ObjectKind) -> RenderShape {
    match kind {
        ObjectKind::PlotBoundary { shape }
        | ObjectKind::Area { shape, .. }
        | ObjectKind::Path { shape, .. }
        | ObjectKind::Linear { shape, .. }
        | ObjectKind::Structure {
            footprint: shape, ..
        } => RenderShape::Path {
            d: shape.to_bez_path().to_svg(),
            closed: shape.closed,
        },
        ObjectKind::Plant {
            position,
            spread,
            symbol,
            category,
            ..
        } => RenderShape::Plant {
            center: *position,
            r: spread / 2.0,
            symbol: *symbol,
            category: *category,
        },
        ObjectKind::Label {
            position,
            text,
            size,
        } => RenderShape::Text {
            at: *position,
            text: text.clone(),
            size: *size,
        },
        ObjectKind::Dimension { from, to, offset } => {
            let (a, b) = dimension_line(*from, *to, *offset);
            RenderShape::Line { from: a, to: b }
        }
    }
}

/// Everything visible, bottom to top.
pub fn scene(doc: &Document, selection: &[ObjectId]) -> Vec<RenderItem> {
    let mut items = Vec::with_capacity(doc.objects.len());
    for o in &doc.objects {
        if !doc.layer(o.layer).is_some_and(|l| l.visible) {
            continue;
        }
        let (def_fill, def_stroke) = default_style(&o.kind);
        let fill = o.style.fill.or(def_fill);
        let stroke = o.style.stroke.or(def_stroke);
        let selected = selection.contains(&o.id);
        let item = |shape, fill, stroke, stroke_width| RenderItem {
            id: o.id,
            shape,
            fill,
            stroke,
            stroke_width,
            selected,
            class: style_class(&o.kind),
        };
        let path = |s: &garden_core::geometry::Shape| RenderShape::Path {
            d: s.to_bez_path().to_svg(),
            closed: s.closed,
        };
        match &o.kind {
            ObjectKind::PlotBoundary { shape } => items.push(item(path(shape), None, stroke, None)),
            ObjectKind::Area { shape, .. } => items.push(item(path(shape), fill, stroke, None)),
            ObjectKind::Structure { footprint, .. } => {
                items.push(item(path(footprint), fill, stroke, None))
            }
            ObjectKind::Path { shape, width } => {
                items.push(item(path(shape), None, stroke, Some(*width)))
            }
            ObjectKind::Linear {
                shape, thickness, ..
            } => items.push(item(path(shape), None, stroke, Some(*thickness))),
            ObjectKind::Plant { .. } => items.push(item(outline(&o.kind), fill, stroke, None)),
            ObjectKind::Label {
                position,
                text,
                size,
            } => items.push(item(
                RenderShape::Text {
                    at: *position,
                    text: text.clone(),
                    size: *size,
                },
                None,
                stroke,
                None,
            )),
            ObjectKind::Dimension { from, to, offset } => {
                let (a, b) = dimension_line(*from, *to, *offset);
                items.push(item(
                    RenderShape::Line { from: a, to: b },
                    None,
                    stroke,
                    None,
                ));
                let text = format_length(from.distance(*to), doc.units);
                items.push(item(
                    RenderShape::Text {
                        at: a.midpoint(b),
                        text,
                        size: 0.0,
                    },
                    None,
                    stroke,
                    None,
                ));
            }
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{doc_with, lawn_rect, plant_at};
    use garden_core::command::Command;
    use garden_core::geometry::Shape;
    use garden_core::model::PlantSymbol;

    #[test]
    fn scene_follows_z_order_and_skips_hidden_layers() {
        let mut d = doc_with(vec![lawn_rect(), plant_at(1000., 1000., 600.)]);
        let s = scene(&d, &[]);
        assert_eq!(s.len(), 2);
        assert!(matches!(s[1].shape, RenderShape::Plant { r, .. } if r == 300.));
        let mut l = d.layers[0].clone();
        l.visible = false;
        d.apply(Command::UpsertLayer { layer: l }, 1).unwrap();
        assert!(scene(&d, &[]).is_empty());
    }

    #[test]
    fn path_uses_its_width_as_stroke() {
        let d = doc_with(vec![ObjectKind::Path {
            shape: Shape::polyline(&[Point::ZERO, Point::new(5000., 0.)], false),
            width: 900.,
        }]);
        let it = &scene(&d, &[])[0];
        assert_eq!(it.stroke_width, Some(900.));
        assert_eq!(it.fill, None);
        assert!(matches!(&it.shape, RenderShape::Path { closed: false, d } if d.starts_with('M')));
    }

    #[test]
    fn dimension_renders_offset_line_and_length_text() {
        let d = doc_with(vec![ObjectKind::Dimension {
            from: Point::ZERO,
            to: Point::new(1000., 0.),
            offset: 500.,
        }]);
        let s = scene(&d, &[]);
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0].shape,
            RenderShape::Line {
                from: Point::new(0., 500.),
                to: Point::new(1000., 500.)
            }
        );
        assert!(matches!(&s[1].shape, RenderShape::Text { text, .. } if text == "1.00 m"));
    }

    #[test]
    fn selection_and_style_override() {
        let mut d = doc_with(vec![lawn_rect()]);
        let id = d.objects[0].id;
        let mut o = d.objects[0].clone();
        o.style.fill = Some(Color(1, 2, 3, 255));
        d.apply(Command::Replace { object: o }, 1).unwrap();
        let it = &scene(&d, &[id])[0];
        assert!(it.selected);
        assert_eq!(it.fill, Some(Color(1, 2, 3, 255)));
    }

    #[test]
    fn css_colour() {
        assert_eq!(color_css(Color(10, 20, 30, 255)), "rgba(10,20,30,1.000)");
    }

    #[test]
    fn plants_render_as_canopy_symbols() {
        let d = doc_with(vec![plant_at(1000., 1000., 600.)]);
        let it = &scene(&d, &[])[0];
        assert_eq!(
            it.shape,
            RenderShape::Plant {
                center: Point::new(1000., 1000.),
                r: 300.,
                symbol: PlantSymbol::Round,
                category: PlantCategory::Shrub
            }
        );
        assert_eq!(it.class, "plant");
    }

    #[test]
    fn items_carry_a_style_class() {
        let classes: Vec<&str> = scene(&doc_with(crate::test_util::all_kinds_sample()), &[])
            .iter()
            .map(|i| i.class)
            .collect();
        assert_eq!(
            classes,
            vec![
                "plot",
                "lawn",
                "path",
                "hedge",
                "structure",
                "plant",
                "label",
                "dimension",
                "dimension"
            ]
        );
    }

    #[test]
    fn plants_are_coloured_by_category() {
        let plant = |category| ObjectKind::Plant {
            position: Point::ZERO,
            spread: 600.,
            height: 500.,
            symbol: PlantSymbol::Round,
            category,
            catalog_id: None,
        };
        let cats = [
            PlantCategory::Tree,
            PlantCategory::Shrub,
            PlantCategory::Perennial,
            PlantCategory::Annual,
            PlantCategory::Grass,
            PlantCategory::Herb,
        ];
        let fills: Vec<_> = cats
            .iter()
            .map(|c| default_style(&plant(*c)).0.unwrap())
            .collect();
        for (i, a) in fills.iter().enumerate() {
            for b in &fills[i + 1..] {
                assert_ne!(
                    (a.0, a.1, a.2),
                    (b.0, b.1, b.2),
                    "categories need distinct colours"
                );
            }
        }
        // tree canopies stay see-through so underplanting shows
        assert!(fills[0].3 < 200);
    }
}
