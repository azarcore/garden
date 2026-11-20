//! Looking after the garden: which objects have plant cards, safe dates and
//! label warnings. Pure; "today" is always passed in.

use garden_core::model::{LinearKind, Object, ObjectKind};

/// Plants and hedges have plant cards; nothing else does.
pub fn is_gardenable(o: &Object) -> bool {
    matches!(
        o.kind,
        ObjectKind::Plant { .. }
            | ObjectKind::Linear {
                kind: LinearKind::Hedge,
                ..
            }
    )
}
