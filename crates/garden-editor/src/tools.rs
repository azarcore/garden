//! Tool kinds and the input events tools react to.

use garden_core::geometry::Point;
use garden_core::model::{AreaType, LinearKind, PlantDef, StructureKind};
use garden_core::query::SnapResult;

use crate::render::RenderShape;

/// What a closed-shape tool creates.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawTarget {
    Plot,
    Area(AreaType),
    Structure(StructureKind),
}

/// What a line tool creates.
#[derive(Clone, Debug, PartialEq)]
pub enum LineTarget {
    Path {
        width: f64,
    },
    Linear {
        kind: LinearKind,
        thickness: f64,
        height: f64,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ToolKind {
    Select,
    Rect(DrawTarget),
    Ellipse(DrawTarget),
    Polygon(DrawTarget),
    Polyline(LineTarget),
    Plant(PlantDef),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Escape,
    Enter,
    Delete,
    Backspace,
}

/// Pointer positions are world mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Down(Point, Modifiers),
    Move(Point, Modifiers),
    Up(Point, Modifiers),
    DoubleClick(Point, Modifiers),
    Key(Key, Modifiers),
}

/// Temporary drawing shown while a gesture is in progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    pub shapes: Vec<RenderShape>,
    /// Live measurement labels: world anchor and text.
    pub dims: Vec<(Point, String)>,
    pub snap: Option<SnapResult>,
    pub marquee: Option<kurbo::Rect>,
}
