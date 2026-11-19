//! Curve geometry: shapes made of line, arc and cubic segments, in millimetres.

mod edit;
mod shape;

pub use edit::{EdgeEditError, SegmentKind};
pub use kurbo::Point;
pub use shape::{PrimitiveHint, Segment, Shape};
