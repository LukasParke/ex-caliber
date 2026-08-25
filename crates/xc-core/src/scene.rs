//! The scene: an ordered set of elements keyed by id.
//!
//! Ordering invariant mirrors Excalidraw: array order equals z-order equals
//! lexicographic order of fractional `index` keys. Every mutation goes through
//! methods that keep ids unique, indices strictly increasing, and bindings
//! referentially consistent — and every mutation records an atomic undo entry.

use std::collections::BTreeMap;

use crate::element::{BoundElementRef, Element};
use crate::findex;
use crate::history::{Change, Entry, History};

/// Scene operation errors.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneError {
    DuplicateId(String),
    UnknownElement(String),
    OrderKey(String),
    Other(String),
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneError::DuplicateId(id) => write!(f, "duplicate element id: {id}"),
            SceneError::UnknownElement(id) => write!(f, "unknown element id: {id}"),
            SceneError::OrderKey(msg) => write!(f, "ordering key error: {msg}"),
            SceneError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SceneError {}

pub type Result<T, E = SceneError> = std::result::Result<T, E>;

fn key_err(r: std::result::Result<String, String>) -> Result<String> {
    r.map_err(SceneError::OrderKey)
}

#[derive(Debug, Clone, Default)]
pub struct Scene {
    elements: BTreeMap<String, Element>,
    /// Raw passthrough of appState we don't model yet (round-trip fidelity).
    pub app_state: serde_json::Value,
    /// Raw passthrough of embedded binary files (dataURLs), keyed by fileId.
    pub files: serde_json::Value,
    pub history: History,
}


impl Scene {
    pub fn new() -> Self {
        Self {
            history: History::new(1000),
            ..Default::default()
        }
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Element> {
        self.elements.get(id)
    }

    /// Non-deleted elements in paint order (back → front).
    pub fn ordered(&self) -> Vec<&Element> {
        let mut v: Vec<&Element> = self.elements.values().filter(|e| !e.isDeleted).collect();
        v.sort_by(|a, b| a.index.cmp(&b.index));
        v
    }

    /// All elements (including tombstones) in arbitrary order; for serialization.
    pub fn elements_iter(&self) -> Vec<&Element> {
        self.elements.values().collect()
    }

    /// Bulk insert used by the file loader: assigns bookkeeping but records no
    /// history (loading a document is not an edit).
    /// Overwrite an element without touching history (live-drag internals).
    pub fn replace_silent(&mut self, el: Element) {
        if self.elements.contains_key(&el.id) {
            self.elements.insert(el.id.clone(), el);
        }
    }

    pub fn add_silent(&mut self, mut el: Element) -> Result<String> {
        self.add_internal(&mut el)?;
        Ok(el.id)
    }

    /// Begin an atomic multi-op transaction; `commit` records ONE undo entry.
    pub fn transaction(&self) -> SceneTx {
        SceneTx::default()
    }

    fn last_live_index(&self) -> Option<String> {
        self.ordered()
            .iter()
            .rev()
            .filter_map(|e| e.index.clone())
            .next()
    }

    /// Add an element (assigning id/seed/index bookkeeping as needed); records undo.
    pub fn add(&mut self, mut el: Element) -> Result<String> {
        let change = self.add_internal(&mut el)?;
        self.history.record(change);
        Ok(el.id)
    }

    /// Internal add used by undo/redo replay (no history recording).
    fn add_internal(&mut self, el: &mut Element) -> Result<Change> {
        if el.id.is_empty() {
            el.id = crate::idgen::new_id();
        }
        if self.elements.contains_key(&el.id) {
            return Err(SceneError::DuplicateId(el.id.clone()));
        }
        el.seed = crate::idgen::new_seed(if el.seed == 0 { None } else { Some(el.seed) });
        el.index = match &el.index {
            Some(ix) if !ix.is_empty() => {
                Some(key_err(findex::generate_key_between(Some(ix), None))?)
            }
            _ => Some(
                key_err(findex::generate_key_between(
                    self.last_live_index().as_deref(),
                    None,
                ))
                .unwrap_or_else(|_| "a0".to_string()),
            ),
        };
        let id = el.id.clone();
        self.elements.insert(id.clone(), el.clone());
        Ok(Change {
            entries: vec![Entry::Insert { id }],
        })
    }

    /// Soft-delete elements (Excalidraw semantics: tombstone, don't remove) and clean
    /// up references: bindings on targets, container labels travel with containers.
    pub fn delete(&mut self, ids: &[String]) -> Result<()> {
        let mut change = Change::default();
        let live: Vec<String> = ids.to_vec();
        // Also tombstone arrows/labels bound to any deleted shape.
        let mut doomed = live.clone();
        for id in &live {
            for el in self.elements.values() {
                if el.isDeleted {
                    continue;
                }
                let bound_to_target = el
                    .startBinding
                    .as_ref()
                    .map(|b| b.element_id() == id)
                    .unwrap_or(false)
                    || el
                        .endBinding
                        .as_ref()
                        .map(|b| b.element_id() == id)
                        .unwrap_or(false);
                let label_of_container = el.containerId.as_deref() == Some(id);
                if (bound_to_target || label_of_container) && !doomed.contains(&el.id) {
                    doomed.push(el.id.clone());
                }
            }
        }
        for id in doomed {
            if let Some(el) = self.elements.get_mut(&id) {
                if el.isDeleted {
                    continue;
                }
                change.entries.push(Entry::Delete {
                    before: el.clone(),
                });
                el.isDeleted = true;
                el.updated = crate::time::now_ms();
            }
        }
        // Drop binding refs pointing at deleted ids.
        let deleted_set: std::collections::HashSet<&String> = ids.iter().collect();
        for el in self.elements.values_mut() {
            if el.isDeleted || el.boundElements.is_none() {
                continue;
            }
            let before = el.clone();
            el.boundElements
                .as_mut()
                .unwrap()
                .retain(|b| !deleted_set.contains(&b.id));
            if el.boundElements.as_ref().unwrap().is_empty() {
                el.boundElements = None;
            }
            if *el != before {
                change.entries.push(Entry::Mutate {
                    before,
                    after: el.clone(),
                });
            }
        }
        if !change.is_empty() {
            self.history.record(change);
        }
        Ok(())
    }

    /// Replace an element wholesale (version bumped centrally); records undo.
    pub fn replace(&mut self, el: Element) -> Result<()> {
        let id = el.id.clone();
        let existing = self
            .elements
            .get_mut(&id)
            .ok_or_else(|| SceneError::UnknownElement(id.clone()))?;
        let mut after = el;
        after.version = existing.version + 1;
        after.updated = crate::time::now_ms();
        // Keep ordering bookkeeping authoritative: index changes go through move ops.
        after.index = existing.index.clone();
        let change = Change {
            entries: vec![Entry::Mutate {
                before: existing.clone(),
                after: after.clone(),
            }],
        };
        *existing = after;
        self.history.record(change);
        Ok(())
    }

    /// Move an element to front/back or relative to another element by regenerating
    /// its fractional key against its new neighbors (`syncMovedIndices` equivalent).
    pub fn reorder(&mut self, id: &str, target: ReorderTarget) -> Result<()> {
        let ordered = self.ordered();
        ordered
            .iter()
            .position(|e| e.id == id)
            .ok_or_else(|| SceneError::UnknownElement(id.to_string()))?;
        if ordered.len() < 2 {
            return Ok(());
        }

        let new_index: String = match &target {
            ReorderTarget::Front => {
                key_err(findex::generate_key_between(
                    self.last_live_index().as_deref(),
                    None,
                ))?
            }
            ReorderTarget::Back => {
                let first = ordered.iter().filter_map(|e| e.index.clone()).next();
                key_err(findex::generate_key_between(None, first.as_deref()))?
            }
            ReorderTarget::Before(other) | ReorderTarget::After(other) => {
                let other_index = self
                    .elements
                    .get(other)
                    .and_then(|e| e.index.clone())
                    .ok_or_else(|| SceneError::UnknownElement(other.clone()))?;
                let (before, after) = if matches!(target, ReorderTarget::Before(_)) {
                    (
                        self.prev_index_before(&other_index),
                        Some(other_index.clone()),
                    )
                } else {
                    (Some(other_index.clone()), self.next_index_after(&other_index))
                };
                key_err(findex::generate_key_between(
                    before.as_deref(),
                    after.as_deref(),
                ))?
            }
        };
        let existing = self.elements.get_mut(id).unwrap();
        let before = existing.clone();
        existing.index = Some(new_index);
        existing.version += 1;
        existing.updated = crate::time::now_ms();
        let after = existing.clone();
        self.history.record(Change {
            entries: vec![Entry::Mutate { before, after }],
        });
        Ok(())
    }

    fn prev_index_before(&self, index: &str) -> Option<String> {
        // Indices ascend in paint order; the last one below `index` is the neighbor.
        self.ordered()
            .iter()
            .filter_map(|e| e.index.clone()).rfind(|ix| ix.as_str() < index)
    }

    fn next_index_after(&self, index: &str) -> Option<String> {
        self.ordered()
            .iter()
            .filter_map(|e| e.index.clone())
            .find(|ix| ix.as_str() > index)
    }

    /// Attach/detach a bound-element ref on a target (used when arrows bind).
    pub fn sync_binding_ref(&mut self, target_id: &str, arrow_id: &str, attach: bool) -> Result<()> {
        let existing = self
            .elements
            .get_mut(target_id)
            .ok_or_else(|| SceneError::UnknownElement(target_id.to_string()))?;
        let before = existing.clone();
        let refs = existing.boundElements.get_or_insert_with(Vec::new);
        if attach && !refs.iter().any(|r| r.id == arrow_id) {
            refs.push(BoundElementRef {
                id: arrow_id.to_string(),
                r#type: "arrow".to_string(),
            });
        } else if !attach {
            refs.retain(|r| r.id != arrow_id);
            if refs.is_empty() {
                existing.boundElements = None;
            }
        }
        if *existing != before {
            let after = existing.clone();
            self.history.record(Change {
                entries: vec![Entry::Mutate { before, after }],
            });
        }
        Ok(())
    }

    // ---- undo/redo ----

    pub fn undo(&mut self) -> bool {
        match self.history.pop_undo() {
            Some(change) => {
                self.apply_inverse(&change);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.history.pop_redo() {
            Some(change) => {
                self.apply_forward(&change);
                true
            }
            None => false,
        }
    }

    fn apply_inverse(&mut self, change: &Change) {
        for entry in change.entries.iter().rev() {
            match entry {
                Entry::Insert { id } => {
                    if let Some(el) = self.elements.get_mut(id) {
                        el.isDeleted = true; // soft-delete preserves redo integrity
                    }
                }
                Entry::Delete { before } => {
                    if let Some(el) = self.elements.get_mut(&before.id) {
                        *el = before.clone();
                    } else {
                        self.elements.insert(before.id.clone(), before.clone());
                    }
                }
                Entry::Mutate { before, .. } => {
                    if let Some(el) = self.elements.get_mut(&before.id) {
                        *el = before.clone();
                    }
                }
            }
        }
    }

    fn apply_forward(&mut self, change: &Change) {
        for entry in &change.entries {
            match entry {
                Entry::Insert { id } => {
                    if let Some(el) = self.elements.get_mut(id) {
                        el.isDeleted = false;
                    }
                }
                Entry::Delete { before } => {
                    if let Some(el) = self.elements.get_mut(&before.id) {
                        el.isDeleted = true;
                    }
                }
                Entry::Mutate { after, .. } => {
                    if let Some(el) = self.elements.get_mut(&after.id) {
                        *el = after.clone();
                    }
                }
            }
        }
    }
}

/// Buffered multi-operation transaction over a scene. Operations mirror the
/// standalone methods but accumulate entries; `commit` records them as a single
/// atomic undo change. Dropping without committing discards the buffer (the
/// scene mutations already applied stay — only history is affected), so always
/// `commit` on the success path.
#[derive(Default)]
pub struct SceneTx {
    entries: Vec<Entry>,
}

impl SceneTx {
    /// Add an element inside the transaction (bookkeeping assigned, no history yet).
    pub fn add(&mut self, scene: &mut Scene, mut el: Element) -> Result<String> {
        let change = scene.add_internal(&mut el)?;
        self.entries.extend(change.entries);
        Ok(el.id)
    }

    /// Apply a whole-element mutation to the scene AND buffer its undo entry.
    pub fn push_mutation(
        &mut self,
        scene: &mut Scene,
        before: Element,
        after: Element,
    ) -> Result<()> {
        if before == after {
            return Ok(());
        }
        if scene.elements.contains_key(&after.id) {
            scene.elements.insert(after.id.clone(), after.clone());
        } else {
            return Err(SceneError::UnknownElement(after.id.clone()));
        }
        self.entries.push(Entry::Mutate { before, after });
        Ok(())
    }

    pub fn sync_binding_ref(
        &mut self,
        scene: &mut Scene,
        target_id: &str,
        arrow_id: &str,
        attach: bool,
    ) -> Result<()> {
        let existing = scene
            .elements
            .get_mut(target_id)
            .ok_or_else(|| SceneError::UnknownElement(target_id.to_string()))?;
        let before = existing.clone();
        let refs = existing.boundElements.get_or_insert_with(Vec::new);
        if attach && !refs.iter().any(|r| r.id == arrow_id) {
            refs.push(BoundElementRef {
                id: arrow_id.to_string(),
                r#type: "arrow".to_string(),
            });
        } else if !attach {
            refs.retain(|r| r.id != arrow_id);
            if refs.is_empty() {
                existing.boundElements = None;
            }
        }
        if existing != &before {
            existing.version += 1;
            existing.updated = crate::time::now_ms();
            let after = existing.clone();
            self.entries.push(Entry::Mutate { before, after });
        }
        Ok(())
    }

    /// Record the buffered entries as ONE undo change.
    pub fn commit(self, scene: &mut Scene) {
        if !self.entries.is_empty() {
            scene.history.record(Change {
                entries: self.entries,
            });
        }
    }
}

#[derive(Debug, Clone)]
pub enum ReorderTarget {
    Front,
    Back,
    Before(String),
    After(String),
}
