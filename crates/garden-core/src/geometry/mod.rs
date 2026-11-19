//! Curve geometry: shapes made of line, arc and cubic segments, in millimetres.

mod shape;

pub use kurbo::Point;
pub use shape::{PrimitiveHint, Segment, Shape};
