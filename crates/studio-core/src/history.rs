//! Undo history as a list of document states.
//!
//! Layers share pixel storage copy-on-write, so a state only costs memory for the layers that
//! changed since the previous one.

use crate::document::{Document, DocState};

pub struct History {
    states: Vec<(String, DocState)>,
    current: usize,
    limit: usize,
}

impl History {
    pub fn new(doc: &Document, label: &str) -> Self {
        History { states: vec![(label.to_string(), doc.state())], current: 0, limit: 60 }
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(2);
    }

    /// Record the document as it is now, after an edit called `label`.
    pub fn commit(&mut self, doc: &Document, label: &str) {
        self.states.truncate(self.current + 1);
        self.states.push((label.to_string(), doc.state()));
        if self.states.len() > self.limit {
            let drop = self.states.len() - self.limit;
            self.states.drain(0..drop);
        }
        self.current = self.states.len() - 1;
    }

    pub fn can_undo(&self) -> bool {
        self.current > 0
    }

    pub fn can_redo(&self) -> bool {
        self.current + 1 < self.states.len()
    }

    /// Step back; returns the label of the undone step.
    pub fn undo(&mut self, doc: &mut Document) -> Option<String> {
        if !self.can_undo() {
            return None;
        }
        let label = self.states[self.current].0.clone();
        self.current -= 1;
        doc.restore(&self.states[self.current].1);
        Some(label)
    }

    pub fn redo(&mut self, doc: &mut Document) -> Option<String> {
        if !self.can_redo() {
            return None;
        }
        self.current += 1;
        doc.restore(&self.states[self.current].1);
        Some(self.states[self.current].0.clone())
    }

    /// Jump straight to state `index` (History panel click).
    pub fn goto(&mut self, doc: &mut Document, index: usize) {
        if index < self.states.len() {
            self.current = index;
            doc.restore(&self.states[index].1);
        }
    }

    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.states.iter().map(|(l, _)| l.as_str())
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.can_undo().then(|| self.states[self.current].0.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.can_redo().then(|| self.states[self.current + 1].0.as_str())
    }

    /// The pristine state's pixels differ from now (unsaved-changes hint).
    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    #[test]
    fn undo_redo_and_branching() {
        let mut doc = Document::new(4, 4, Some(Rgba::WHITE));
        let mut h = History::new(&doc, "New");
        doc.add_empty_layer("A");
        h.commit(&doc, "New Layer");
        doc.add_empty_layer("B");
        h.commit(&doc, "New Layer");
        assert_eq!(doc.layers.len(), 3);
        assert_eq!(h.undo(&mut doc).as_deref(), Some("New Layer"));
        assert_eq!(doc.layers.len(), 2);
        h.redo(&mut doc);
        assert_eq!(doc.layers.len(), 3);
        h.goto(&mut doc, 0);
        assert_eq!(doc.layers.len(), 1);
        doc.add_empty_layer("C");
        h.commit(&doc, "New Layer");
        assert!(!h.can_redo());
        assert_eq!(h.len(), 2);
    }
}
