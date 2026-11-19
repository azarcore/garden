//! A document plus its undo/redo history.

use crate::command::{Command, CommandError};
use crate::model::Document;

pub struct Editor {
    doc: Document,
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl Editor {
    pub fn new(doc: Document) -> Editor {
        Editor { doc, undo: Vec::new(), redo: Vec::new() }
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// Apply a user edit. A failed command changes nothing, history included.
    pub fn execute(&mut self, cmd: Command, now_ms: i64) -> Result<(), CommandError> {
        let inverse = self.doc.apply(cmd, now_ms)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }

    /// Returns false if there was nothing to undo.
    pub fn undo(&mut self, now_ms: i64) -> bool {
        Self::step(&mut self.doc, &mut self.undo, &mut self.redo, now_ms)
    }

    /// Returns false if there was nothing to redo.
    pub fn redo(&mut self, now_ms: i64) -> bool {
        Self::step(&mut self.doc, &mut self.redo, &mut self.undo, now_ms)
    }

    fn step(doc: &mut Document, from: &mut Vec<Command>, to: &mut Vec<Command>, now_ms: i64) -> bool {
        let Some(cmd) = from.pop() else { return false };
        match doc.apply(cmd.clone(), now_ms) {
            Ok(inverse) => {
                to.push(inverse);
                true
            }
            Err(_) => {
                from.push(cmd);
                false
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn into_document(self) -> Document {
        self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;
    use crate::model::{Object, ObjectId, ObjectKind};

    #[test]
    fn undo_redo_round_trip() {
        let doc = Document::new("g", 0);
        let l = doc.layers[0].id;
        let mut e = Editor::new(doc);
        let o = Object::new("x", ObjectKind::Label { position: Point::ZERO, text: "x".into(), size: 1. }, l);
        e.execute(Command::Insert { object: o.clone(), index: 0 }, 1).unwrap();
        assert!(e.undo(2));
        assert!(e.doc().objects.is_empty());
        assert!(e.redo(3));
        assert_eq!(e.doc().objects[0].id, o.id);
        assert!(!e.redo(4));
    }

    #[test]
    fn new_command_clears_redo() {
        let mut e = Editor::new(Document::new("g", 0));
        e.execute(Command::Rename { name: "a".into() }, 1).unwrap();
        e.undo(2);
        assert!(e.can_redo());
        e.execute(Command::Rename { name: "b".into() }, 3).unwrap();
        assert!(!e.can_redo());
        assert_eq!(e.doc().name, "b");
    }

    #[test]
    fn failed_execute_leaves_history_alone() {
        let mut e = Editor::new(Document::new("g", 0));
        assert!(e.execute(Command::Delete { id: ObjectId::new() }, 1).is_err());
        assert!(!e.can_undo());
    }
}
