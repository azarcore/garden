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
    /// Two clicks on the traced photo, then a typed real distance.
    SetScale,
    /// A click places a text label.
    Text,
    /// Two clicks place a dimension line.
    Dimension,
    /// Two clicks show a distance; nothing is stored.
    Measure,
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
    /// Smart guides (alignment, spacing, distances) to draw.
    pub guides: Vec<crate::guides::GuideLine>,
}

/// One line telling the person what to do next with `tool`.
pub fn hint(
    tool: &ToolKind,
    points_placed: usize,
    has_background: bool,
    s: &crate::strings::Strings,
) -> &'static str {
    match tool {
        ToolKind::SetScale if !has_background => s.hint_scale_need_photo,
        ToolKind::SetScale => match points_placed {
            0 => s.hint_scale_first,
            1 => s.hint_scale_second,
            _ => s.hint_scale_type,
        },
        ToolKind::Select => s.hint_select,
        ToolKind::Rect(_) | ToolKind::Ellipse(_) => s.hint_drag,
        ToolKind::Polygon(_) if points_placed == 0 => s.hint_shape_start,
        ToolKind::Polygon(_) => s.hint_shape_more,
        ToolKind::Polyline(_) if points_placed == 0 => s.hint_line_start,
        ToolKind::Polyline(_) => s.hint_line_more,
        ToolKind::Plant(_) => s.hint_plant,
        ToolKind::Text => s.hint_text,
        ToolKind::Dimension if points_placed == 0 => s.hint_dim_first,
        ToolKind::Dimension => s.hint_dim_second,
        ToolKind::Measure => s.hint_measure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::EN;
    use garden_core::model::AreaType;

    #[test]
    fn hints_explain_how_to_use_each_tool() {
        let line = ToolKind::Polyline(LineTarget::Path { width: 900. });
        assert_eq!(hint(&line, 0, false, &EN), EN.hint_line_start);
        assert_eq!(hint(&line, 2, false, &EN), EN.hint_line_more);
        let bed = ToolKind::Polygon(DrawTarget::Area(AreaType::Bed));
        assert_eq!(hint(&bed, 0, false, &EN), EN.hint_shape_start);
        assert_eq!(hint(&bed, 3, false, &EN), EN.hint_shape_more);
        assert_eq!(
            hint(&ToolKind::Rect(DrawTarget::Plot), 0, false, &EN),
            EN.hint_drag
        );
        assert_eq!(hint(&ToolKind::Select, 0, false, &EN), EN.hint_select);
        assert_eq!(
            hint(&ToolKind::SetScale, 0, false, &EN),
            EN.hint_scale_need_photo
        );
        assert_eq!(hint(&ToolKind::SetScale, 0, true, &EN), EN.hint_scale_first);
        assert_eq!(
            hint(&ToolKind::SetScale, 1, true, &EN),
            EN.hint_scale_second
        );
        assert_eq!(hint(&ToolKind::SetScale, 2, true, &EN), EN.hint_scale_type);
        assert_eq!(hint(&ToolKind::Text, 0, false, &EN), EN.hint_text);
        assert_eq!(hint(&ToolKind::Dimension, 0, false, &EN), EN.hint_dim_first);
        assert_eq!(
            hint(&ToolKind::Dimension, 1, false, &EN),
            EN.hint_dim_second
        );
        assert_eq!(hint(&ToolKind::Measure, 2, false, &EN), EN.hint_measure);
    }
}
