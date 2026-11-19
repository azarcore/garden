#![forbid(unsafe_code)]
//! Browser-independent editor logic: viewport, scene, tools, inspector.

pub mod render;
pub mod strings;
pub mod viewport;

#[cfg(test)]
pub(crate) mod test_util {
    use garden_core::command::Command;
    use garden_core::geometry::{Point, Shape};
    use garden_core::model::*;

    pub fn doc_with(kinds: Vec<ObjectKind>) -> Document {
        let mut d = Document::new("g", 0);
        let l = d.layers[0].id;
        for (i, k) in kinds.into_iter().enumerate() {
            d.apply(
                Command::Insert {
                    object: Object::new(format!("{i}"), k, l),
                    index: i,
                },
                0,
            )
            .unwrap();
        }
        d
    }

    pub fn plant_at(x: f64, y: f64, spread: f64) -> ObjectKind {
        ObjectKind::Plant {
            position: Point::new(x, y),
            spread,
            height: 500.,
            symbol: PlantSymbol::Round,
            category: PlantCategory::Shrub,
            catalog_id: None,
        }
    }

    pub const NO: crate::tools::Modifiers = crate::tools::Modifiers {
        shift: false,
        alt: false,
        ctrl: false,
    };
    pub const SHIFT: crate::tools::Modifiers = crate::tools::Modifiers {
        shift: true,
        alt: false,
        ctrl: false,
    };

    /// Editor on a 1000×500 px view at the default scale: 1 px = 20 mm,
    /// snap radius 160 mm, grid 100 mm.
    pub fn state_with(kinds: Vec<ObjectKind>) -> crate::state::EditorState {
        let mut s = crate::state::EditorState::new(doc_with(kinds), 1000., 500.);
        s.viewport = crate::viewport::Viewport::new(1000., 500.);
        s
    }

    pub fn click(s: &mut crate::state::EditorState, p: Point, m: crate::tools::Modifiers) {
        s.handle(crate::tools::Input::Down(p, m), 1);
        s.handle(crate::tools::Input::Up(p, m), 1);
    }

    pub fn lawn_rect() -> ObjectKind {
        ObjectKind::Area {
            area_type: AreaType::Lawn,
            shape: Shape::rect(Point::ZERO, 4000., 3000.),
        }
    }

    /// One of every object kind.
    pub fn all_kinds_sample() -> Vec<ObjectKind> {
        let line = Shape::polyline(&[Point::ZERO, Point::new(1000., 0.)], false);
        vec![
            ObjectKind::PlotBoundary {
                shape: Shape::rect(Point::ZERO, 10_000., 10_000.),
            },
            lawn_rect(),
            ObjectKind::Path {
                shape: line.clone(),
                width: 900.,
            },
            ObjectKind::Linear {
                kind: LinearKind::Hedge,
                shape: line,
                thickness: 600.,
                height: 1500.,
            },
            ObjectKind::Structure {
                kind: StructureKind::Shed,
                footprint: Shape::rect(Point::ZERO, 2000., 1500.),
                height: 2200.,
            },
            plant_at(100., 100., 600.),
            ObjectKind::Label {
                position: Point::ZERO,
                text: "Hi".into(),
                size: 300.,
            },
            ObjectKind::Dimension {
                from: Point::ZERO,
                to: Point::new(1000., 0.),
                offset: 500.,
            },
        ]
    }
}
pub mod catalog;
mod draw;
pub mod objects;
pub mod state;
pub mod tools;
