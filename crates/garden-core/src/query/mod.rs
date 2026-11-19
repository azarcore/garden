//! Read-only questions about a document: what is under the cursor, where to snap.

mod hit;
mod snap;

pub use hit::hit_test;
pub use snap::{SnapKind, SnapResult, SnapSettings, snap, snap_angle};

#[cfg(test)]
pub(crate) mod test_util {
    use crate::command::Command;
    use crate::model::{Document, Object, ObjectKind};

    pub fn doc_with(objs: Vec<ObjectKind>) -> Document {
        let mut d = Document::new("g", 0);
        let l = d.layers[0].id;
        for (i, k) in objs.into_iter().enumerate() {
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
}
