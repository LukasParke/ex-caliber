//! Editing operations: move, resize, duplicate, group/ungroup, align — all
//! binding-aware and transactional.
//!
//! Binding recompute semantics (fixedPoint contract): an arrow's anchor stays at
//! the same *ratio* of its target's bbox, so moving or resizing a shape moves
//! attached arrow endpoints automatically; container labels re-center in their
//! container. Elbow-arrow routing is preserved-not-recomputed (M7).

use crate::element::{Binding, Element, ElementType};
use crate::idgen;
use crate::scene::{Result, Scene};

/// Move elements by a delta. Attached arrows re-anchor; container labels travel.
pub fn move_elements(scene: &mut Scene, ids: &[String], dx: f64, dy: f64) -> Result<()> {
    let moved: std::collections::HashSet<String> = ids.iter().cloned().collect();
    let mut tx = crate::scene::SceneTx::default();

    // 1. Translate the moved elements themselves.
    for id in ids {
        translate_one(scene, id, dx, dy, &mut tx)?;
    }
    // 2. Recompute arrows bound to anything in the moved set.
    let bound_arrows = arrows_bound_to(scene, &moved);
    for arrow_id in bound_arrows {
        reanchor_arrow(scene, &arrow_id, Some(&moved), &mut tx)?;
    }
    // 3. Labels inside moved containers re-center.
    for label in labels_in_containers(scene, &moved) {
        recenter_label(scene, &label, &moved, &mut tx)?;
    }
    tx.commit(scene);
    Ok(())
}

/// Resize an element to a new bbox (top-left + size). Non-uniform scale of linear
/// point sets matches excalidraw's behavior of scaling point coordinates.
pub fn resize_element(scene: &mut Scene, id: &str, x: f64, y: f64, w: f64, h: f64) -> Result<()> {
    let mut tx = crate::scene::SceneTx::default();
    let el = scene
        .get(id)
        .cloned()
        .ok_or_else(|| crate::scene::SceneError::UnknownElement(id.to_string()))?;
    let (old_w, old_h) = el.effective_size();
    let mut next = el.clone();
    next.x = x;
    next.y = y;
    if next.is_linear() {
        if let Some(points) = &mut next.points {
            let sx = if old_w != 0.0 { w / old_w } else { 1.0 };
            let sy = if old_h != 0.0 { h / old_h } else { 1.0 };
            for p in points.iter_mut() {
                p[0] *= sx;
                p[1] *= sy;
            }
        }
    } else {
        next.width = w;
        next.height = h;
    }
    apply_mutation(scene, &mut tx, next)?;
    let moved: std::collections::HashSet<String> = [id.to_string()].into_iter().collect();
    for arrow_id in arrows_bound_to(scene, &moved) {
        reanchor_arrow(scene, &arrow_id, Some(&moved), &mut tx)?;
    }
    for label in labels_in_containers(scene, &moved) {
        recenter_label(scene, &label, &moved, &mut tx)?;
        refit_label_font(scene, &label, id, &mut tx)?;
    }
    tx.commit(scene);
    Ok(())
}

/// Shrink a container label's font until it fits the container width
/// (excalidraw's auto-fit); floor at 8px.
fn refit_label_font(
    scene: &mut Scene,
    label_id: &str,
    container_id: &str,
    tx: &mut SceneTx,
) -> Result<()> {
    let engine = crate::text::global_engine();
    let (Some(label), Some(container)) = (
        scene.get(label_id).cloned(),
        scene.get(container_id).cloned(),
    ) else {
        return Ok(());
    };
    let (cw, _) = container.effective_size();
    let inner_w = (cw - 10.0).max(8.0); // BOUND_TEXT_PADDING ≈ 5 each side
    let font_size = label.fontSize.unwrap_or(20.0);
    let lh = label.lineHeight.unwrap_or(1.25);
    let family = crate::text::family_for(label.fontFamily.unwrap_or(1));
    let text = label.text.as_deref().unwrap_or("");
    let mut size = font_size;
    let mut measured = engine.measure(text, family, size, lh).0;
    while measured > inner_w && size > 8.0 {
        size = (size - 1.0).max(8.0);
        measured = engine.measure(text, family, size, lh).0;
    }
    if (size - font_size).abs() < 0.5 {
        return Ok(());
    }
    let mut next = label.clone();
    next.fontSize = Some(size);
    next.height = (next.height / font_size) * size;
    apply_mutation(scene, tx, next)
}

/// Duplicate elements in place (fresh ids/seeds, stacked on top of originals).
/// Returns the new ids in input order.
pub fn duplicate(scene: &mut Scene, ids: &[String]) -> Result<Vec<String>> {
    let mut tx = crate::scene::SceneTx::default();
    let mut new_ids = Vec::with_capacity(ids.len());
    for id in ids {
        let mut clone = scene
            .get(id)
            .cloned()
            .ok_or_else(|| crate::scene::SceneError::UnknownElement(id.to_string()))?;
        clone.id = idgen::new_id();
        clone.seed = idgen::new_seed(None);
        clone.versionNonce = idgen::new_seed(None);
        clone.version = 1;
        // Bindings pointing at the originals stay pointing there (excalidraw does
        // the same for plain duplicate); boundElements of the clone are dropped —
        // arrows belong to the original relationship, not the copy.
        clone.boundElements = None;
        let new_id = tx.add(scene, clone)?;
        new_ids.push(new_id);
    }
    tx.commit(scene);
    Ok(new_ids)
}

/// Put all ids into one new group (outermost = last in each groupIds list).
pub fn group(scene: &mut Scene, ids: &[String]) -> Result<String> {
    if ids.len() < 2 {
        return Err(crate::scene::SceneError::Other(
            "grouping requires at least 2 elements".into(),
        ));
    }
    let gid = idgen::new_id();
    let mut tx = crate::scene::SceneTx::default();
    for id in ids {
        let el = scene
            .get(id)
            .cloned()
            .ok_or_else(|| crate::scene::SceneError::UnknownElement(id.to_string()))?;
        let mut next = el.clone();
        if !next.groupIds.contains(&gid) {
            next.groupIds.push(gid.clone()); // shallowest last
        }
        apply_mutation(scene, &mut tx, next)?;
    }
    tx.commit(scene);
    Ok(gid)
}

/// Remove the deepest common group across the ids.
pub fn ungroup(scene: &mut Scene, ids: &[String]) -> Result<Option<String>> {
    let common = common_group(scene, ids)
        .ok_or_else(|| crate::scene::SceneError::Other("no common group".into()))?;
    let mut tx = crate::scene::SceneTx::default();
    for id in ids {
        if let Some(el) = scene.get(id).cloned() {
            let mut next = el.clone();
            next.groupIds.retain(|g| g != &common);
            apply_mutation(scene, &mut tx, next)?;
        }
    }
    tx.commit(scene);
    Ok(Some(common))
}

/// Deepest group id present on every id.
pub fn common_group(scene: &Scene, ids: &[String]) -> Option<String> {
    let mut iter = ids.iter();
    let first = scene.get(iter.next()?)?;
    let candidate = first.groupIds.first()?;
    if ids.iter().all(|id| {
        scene
            .get(id)
            .map(|e| e.groupIds.contains(candidate))
            .unwrap_or(false)
    }) {
        Some(candidate.clone())
    } else {
        None
    }
}

/// Align elements within their common bounding box.
pub fn align(scene: &mut Scene, ids: &[String], mode: AlignMode) -> Result<()> {
    if ids.len() < 2 {
        return Err(crate::scene::SceneError::Other(
            "alignment requires at least 2 elements".into(),
        ));
    }
    let els: Vec<Element> = ids
        .iter()
        .map(|id| scene.get(id).cloned())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| crate::scene::SceneError::UnknownElement("<missing>".into()))?;
    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    for el in &els {
        let (w, h) = el.effective_size();
        min_x = min_x.min(el.x);
        min_y = min_y.min(el.y);
        max_x = max_x.max(el.x + w);
        max_y = max_y.max(el.y + h);
    }
    let mut tx = crate::scene::SceneTx::default();
    for el in &els {
        let (w, h) = el.effective_size();
        let (nx, ny) = match mode {
            AlignMode::Left => (min_x, el.y),
            AlignMode::Right => (max_x - w, el.y),
            AlignMode::CenterX => ((min_x + max_x - w) / 2.0, el.y),
            AlignMode::Top => (el.x, min_y),
            AlignMode::Bottom => (el.x, max_y - h),
            AlignMode::CenterY => (el.x, (min_y + max_y - h) / 2.0),
        };
        if (nx - el.x).abs() > f64::EPSILON || (ny - el.y).abs() > f64::EPSILON {
            translate_one(scene, &el.id, nx - el.x, ny - el.y, &mut tx)?;
        }
    }
    // Alignment moves things: bound arrows re-anchor for the whole aligned set.
    let moved: std::collections::HashSet<String> = ids.iter().cloned().collect();
    for arrow_id in arrows_bound_to(scene, &moved) {
        reanchor_arrow(scene, &arrow_id, Some(&moved), &mut tx)?;
    }
    for label in labels_in_containers(scene, &moved) {
        recenter_label(scene, &label, &moved, &mut tx)?;
    }
    tx.commit(scene);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    Right,
    CenterX,
    Top,
    Bottom,
    CenterY,
}

// ---- internals ----

fn apply_mutation(scene: &mut Scene, tx: &mut SceneTx, next: Element) -> Result<()> {
    let current = scene
        .get(&next.id)
        .cloned()
        .ok_or_else(|| crate::scene::SceneError::UnknownElement(next.id.clone()))?;
    let mut next = next;
    next.version = current.version + 1;
    next.updated = crate::time::now_ms();
    // Ordering bookkeeping is owned by reorder ops.
    next.index = current.index.clone();
    tx.push_mutation(scene, current, next)
}

fn translate_one(scene: &mut Scene, id: &str, dx: f64, dy: f64, tx: &mut SceneTx) -> Result<()> {
    let el = scene
        .get(id)
        .cloned()
        .ok_or_else(|| crate::scene::SceneError::UnknownElement(id.to_string()))?;
    let mut next = el.clone();
    next.x += dx;
    next.y += dy;
    apply_mutation(scene, tx, next)
}

fn arrows_bound_to(scene: &Scene, moved: &std::collections::HashSet<String>) -> Vec<String> {
    // Prefer the boundElements index; fall back to a scan (files may be stale).
    let mut out: Vec<String> = Vec::new();
    for id in moved {
        if let Some(refs) = scene.get(id).and_then(|el| el.boundElements.as_ref()) {
            for r in refs {
                if r.r#type == "arrow" {
                    out.push(r.id.clone());
                }
            }
        }
    }
    if out.is_empty() {
        for el in scene.elements_iter() {
            if el.kind != ElementType::Arrow || el.isDeleted {
                continue;
            }
            let touches = |b: &Option<Binding>| {
                b.as_ref()
                    .map(|b| moved.contains(b.element_id()))
                    .unwrap_or(false)
            };
            if touches(&el.startBinding) || touches(&el.endBinding) {
                out.push(el.id.clone());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn labels_in_containers(scene: &Scene, moved: &std::collections::HashSet<String>) -> Vec<String> {
    scene
        .elements_iter()
        .into_iter()
        .filter(|e| {
            e.kind == ElementType::Text
                && !e.isDeleted
                && e.containerId
                    .as_ref()
                    .map(|c| moved.contains(c))
                    .unwrap_or(false)
        })
        .map(|e| e.id.clone())
        .collect()
}

/// Recompute one arrow's bound endpoints against its targets' current bboxes.
/// Unbound endpoints stay put.
fn reanchor_arrow(
    scene: &mut Scene,
    arrow_id: &str,
    moved: Option<&std::collections::HashSet<String>>,
    tx: &mut SceneTx,
) -> Result<()> {
    let arrow = scene
        .get(arrow_id)
        .cloned()
        .ok_or_else(|| crate::scene::SceneError::UnknownElement(arrow_id.to_string()))?;
    // User-pinned segments are preserved verbatim; unpinned elbows re-route.
    let pinned_elbow = arrow.elbowed == Some(true)
        && arrow
            .fixedSegments
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);
    if pinned_elbow {
        return Ok(());
    }
    let points = match arrow.points.clone() {
        Some(p) => p,
        None => return Ok(()),
    };
    if points.is_empty() {
        return Ok(());
    }

    let mut next = arrow.clone();
    let pts = next.points.as_mut().unwrap();
    let mut origin = (next.x, next.y);

    let recompute_end = |end_binding: &Option<Binding>,
                         idx: usize,
                         pts: &mut Vec<[f64; 2]>,
                         origin: &mut (f64, f64),
                         moved: Option<&std::collections::HashSet<String>>|
     -> Result<bool> {
        let Some(binding) = end_binding else {
            return Ok(false);
        };
        let target = scene.get(binding.element_id()).cloned().ok_or_else(|| {
            crate::scene::SceneError::UnknownElement(binding.element_id().to_string())
        })?;
        // Only re-anchor when THIS end's target moved (or recompute forced).
        if let Some(moved) = moved.filter(|m| !m.contains(binding.element_id())) {
            let _ = moved;
            return Ok(false);
        }
        let (tw, th) = target.effective_size();
        let fp = match binding {
            Binding::Fixed(b) => b.fixed_point,
            Binding::Legacy(_) | Binding::Other(_) => [0.5, 0.5], // legacy: aim at center
        };
        let (fx, fy) = (fp[0], fp[1]);
        let world = [target.x + fx * tw, target.y + fy * th];
        // Express the new endpoint relative to the arrow origin; if it escapes
        // the negative quadrant excalidraw-style, shift the origin instead.
        let rel = [world[0] - origin.0, world[1] - origin.1];
        if rel[0] < 0.0 || rel[1] < 0.0 {
            origin.0 += rel[0].min(0.0);
            origin.1 += rel[1].min(0.0);
            for p in pts.iter_mut() {
                p[0] -= rel[0].min(0.0);
                p[1] -= rel[1].min(0.0);
            }
            let rel = [world[0] - origin.0, world[1] - origin.1];
            pts[idx] = rel;
        } else {
            pts[idx] = rel;
        }
        Ok(true)
    };

    let start_changed = recompute_end(&next.startBinding, 0, pts, &mut origin, moved)?;
    let end_idx = pts.len() - 1;
    let end_changed = recompute_end(&next.endBinding, end_idx, pts, &mut origin, moved)?;

    if next.elbowed == Some(true) && (start_changed || end_changed) {
        // Re-route the elbow between the (possibly moved) endpoints.
        let route = crate::router::route_elbow(
            [origin.0 + pts[0][0], origin.1 + pts[0][1]],
            [origin.0 + pts[end_idx][0], origin.1 + pts[end_idx][1]],
        );
        // Express the route relative to a normalized origin.
        let (min_x, min_y) = (
            route.iter().map(|p| p[0]).fold(f64::MAX, f64::min),
            route.iter().map(|p| p[1]).fold(f64::MAX, f64::min),
        );
        origin = (min_x, min_y);
        *pts = route
            .into_iter()
            .map(|[x, y]| [x - min_x, y - min_y])
            .collect();
    }

    if start_changed || end_changed {
        let current = scene
            .get(arrow_id)
            .cloned()
            .ok_or_else(|| crate::scene::SceneError::UnknownElement(arrow_id.to_string()))?;
        next.x = origin.0;
        next.y = origin.1;
        let (w, h) = {
            let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
            let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
            for [px, py] in pts.iter() {
                min_x = min_x.min(*px);
                min_y = min_y.min(*py);
                max_x = max_x.max(*px);
                max_y = max_y.max(*py);
            }
            (max_x - min_x, max_y - min_y)
        };
        next.width = w;
        next.height = h;
        next.version = current.version + 1;
        next.updated = crate::time::now_ms();
        next.index = current.index.clone();
        tx.push_mutation(scene, current, next)?;
    }
    Ok(())
}

fn recenter_label(
    scene: &mut Scene,
    label_id: &str,
    moved: &std::collections::HashSet<String>,
    tx: &mut SceneTx,
) -> Result<()> {
    let label = scene
        .get(label_id)
        .cloned()
        .ok_or_else(|| crate::scene::SceneError::UnknownElement(label_id.to_string()))?;
    let Some(container_id) = label.containerId.clone() else {
        return Ok(());
    };
    if !moved.contains(&container_id) {
        return Ok(());
    }
    let container = scene
        .get(&container_id)
        .cloned()
        .ok_or_else(|| crate::scene::SceneError::UnknownElement(container_id.clone()))?;
    let (cw, ch) = container.effective_size();
    let (lw, lh) = label.effective_size();
    let mut next = label.clone();
    next.x = container.x + (cw - lw) / 2.0;
    next.y = container.y + (ch - lh) / 2.0;
    apply_mutation(scene, tx, next)
}

use crate::scene::SceneTx;

// ---- silent drag support (live manipulation without history spam) ----

/// Snapshot taken when a drag starts: every element that might change.
pub struct DragSession {
    ids: Vec<String>,
    before: Vec<Element>,
}

impl DragSession {
    /// Current committed state of a tracked element (if still present).
    pub fn before_of(&self, id: &str) -> Option<&Element> {
        self.before.iter().find(|e| e.id == id)
    }
}

/// Collect `ids` plus their bound arrows and container labels, snapshotting
/// before-states. Call once on drag start.
pub fn begin_drag(scene: &Scene, ids: &[String]) -> DragSession {
    let moved: std::collections::HashSet<String> = ids.iter().cloned().collect();
    let mut all: Vec<String> = ids.to_vec();
    all.extend(arrows_bound_to(scene, &moved));
    all.extend(labels_in_containers(scene, &moved));
    all.sort();
    all.dedup();
    let before = all.iter().filter_map(|id| scene.get(id).cloned()).collect();
    DragSession { ids: all, before }
}

fn silent_replace(scene: &mut Scene, next: Element) {
    scene.replace_silent(next);
}

/// Apply the drag delta relative to drag-start (idempotent: state is reset to
/// the session's before-values first). No history is touched.
pub fn drag_move(scene: &mut Scene, session: &DragSession, dx: f64, dy: f64) {
    // Reset to before-states, then translate the moved set.
    for before in &session.before {
        silent_replace(scene, before.clone());
    }
    // Reset-to-before then translate everything: arrows re-anchor and labels
    // re-center below, so translating connected elements is equivalent.
    for before in &session.before {
        let mut next = before.clone();
        next.x += dx;
        next.y += dy;
        silent_replace(scene, next);
    }
    // Re-anchor every arrow in the session against current target positions.
    for id in &session.ids {
        if let Some(arrow) = scene
            .get(id)
            .cloned()
            .filter(|a| a.kind == ElementType::Arrow)
        {
            reanchor_arrow_silent(scene, &arrow);
        }
    }
    // Re-center labels.
    for id in &session.ids {
        let label = scene
            .get(id)
            .cloned()
            .filter(|l| l.kind == ElementType::Text);
        if let Some(label) = label {
            let container = label
                .containerId
                .clone()
                .and_then(|cid| scene.get(&cid).cloned());
            if let Some(container) = container {
                let (cw, ch) = container.effective_size();
                let (lw, lh) = label.effective_size();
                let mut next = label;
                next.x = container.x + (cw - lw) / 2.0;
                next.y = container.y + (ch - lh) / 2.0;
                silent_replace(scene, next);
            }
        }
    }
}

/// Record the drag as ONE undo change (befores vs current states).
pub fn end_drag(scene: &mut Scene, session: DragSession) {
    let mut entries = Vec::new();
    for before in session.before {
        if let Some(after) = scene.get(&before.id).cloned().filter(|a| *a != before) {
            entries.push(crate::history::Entry::Mutate { before, after });
        }
    }
    if !entries.is_empty() {
        scene.history.record(crate::history::Change { entries });
    }
}

fn reanchor_arrow_silent(scene: &mut Scene, arrow: &Element) {
    let mut next = arrow.clone();
    let Some(points) = next.points.as_mut() else {
        return;
    };
    if points.is_empty() {
        return;
    }
    let mut origin = (next.x, next.y);
    for (idx, binding) in [
        (0usize, &next.startBinding),
        (points.len() - 1, &next.endBinding),
    ] {
        let Some(b) = binding else { continue };
        let Some(target) = scene.get(b.element_id()) else {
            continue;
        };
        let (tw, th) = target.effective_size();
        let fp = match b {
            Binding::Fixed(fb) => fb.fixed_point,
            _ => [0.5, 0.5],
        };
        let world = [target.x + fp[0] * tw, target.y + fp[1] * th];
        let rel = [world[0] - origin.0, world[1] - origin.1];
        if rel[0] < 0.0 || rel[1] < 0.0 {
            let (sx, sy) = (rel[0].min(0.0), rel[1].min(0.0));
            origin.0 += sx;
            origin.1 += sy;
            for p in points.iter_mut() {
                p[0] -= sx;
                p[1] -= sy;
            }
            points[idx] = [world[0] - origin.0, world[1] - origin.1];
        } else {
            points[idx] = rel;
        }
    }
    next.x = origin.0;
    next.y = origin.1;
    silent_replace(scene, next);
}
