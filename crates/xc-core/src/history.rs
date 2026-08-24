//! Command-pattern undo/redo over whole-scene changes.
//!
//! A `Change` is a transaction of per-element inverse entries, so composite ops
//! (e.g., "delete selection" touching bound arrows) undo atomically. MCP mutations
//! and UI actions both funnel through `Scene` methods that record here — one shared
//! history for human and agent edits (plan §7).

/// One element's before/after within a change.
// Insert carries only an id while Mutate snapshots whole elements; the size
// difference is the point — full snapshots make undo trivially correct.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Entry {
    /// Element added; undo removes it.
    Insert { id: String },
    /// Element removed; undo restores it.
    Delete { before: crate::element::Element },
    /// Mutation with full before/after snapshots.
    Mutate {
        before: crate::element::Element,
        after: crate::element::Element,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Change {
    pub entries: Vec<Entry>,
}

impl Change {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct History {
    undo_stack: Vec<Change>,
    redo_stack: Vec<Change>,
    limit: usize,
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            limit,
        }
    }

    pub fn record(&mut self, change: Change) {
        if change.is_empty() {
            return;
        }
        self.undo_stack.push(change);
        self.redo_stack.clear();
        if self.limit > 0 && self.undo_stack.len() > self.limit {
            self.undo_stack.remove(0);
        }
    }

    pub fn pop_undo(&mut self) -> Option<Change> {
        let c = self.undo_stack.pop()?;
        self.redo_stack.push(c.clone());
        Some(c)
    }

    pub fn pop_redo(&mut self) -> Option<Change> {
        let c = self.redo_stack.pop()?;
        self.undo_stack.push(c.clone());
        Some(c)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}
