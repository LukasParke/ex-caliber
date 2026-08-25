//! Hit testing: point → element resolution in world space.
//!
//! Topmost-first (paint order reversed). Rotation-aware: points are tested
//! against the element's un-rotated bounding box by inverse-rotating the query
//! point around the element center.

use crate::element::{Element, ElementType};

/// Does `point` hit `el`? Uses the effective bbox; linear elements use their
/// point hull (already reflected in `effective_size` with origin at min point —
/// note: excalidraw linear elements keep x,y as the *first point*, so the hull
/// can extend negatively; we test against the actual point hull when present).
pub fn hits(el: &Element, px: f64, py: f64) -> bool {
    let slop = 4.0;
    if let Some(points) = el.points.as_deref().filter(|_| el.is_linear()) {
        {
            if points.len() == 1 {
                let [x0, y0] = points[0];
                return (el.x + x0 - px).abs() <= slop && (el.y + y0 - py).abs() <= slop;
            }
            // Distance-to-segment test against the absolute polyline.
            for w in points.windows(2) {
                if point_near_segment(el.x, el.y, w[0], w[1], px, py, slop + el.strokeWidth / 2.0) {
                    return true;
                }
            }
            return false;
        }
    }
    let (w, h) = el.effective_size();
    let (mut min_x, mut min_y) = (el.x, el.y);
    let (mut max_x, mut max_y) = (el.x + w, el.y + h);
    if max_x < min_x {
        std::mem::swap(&mut min_x, &mut max_x);
    }
    if max_y < min_y {
        std::mem::swap(&mut min_y, &mut max_y);
    }
    let (cx, cy) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    // Inverse-rotate the query point about the center.
    let (dx, dy) = (px - cx, py - cy);
    let (s, c) = (-el.angle).sin_cos();
    let (rx, ry) = (dx * c - dy * s, dx * s + dy * c);
    let (qx, qy) = (cx + rx, cy + ry);
    qx >= min_x - slop && qx <= max_x + slop && qy >= min_y - slop && qy <= max_y + slop
}

fn point_near_segment(ox: f64, oy: f64, a: [f64; 2], b: [f64; 2], px: f64, py: f64, slop: f64) -> bool {
    let (ax, ay) = (ox + a[0], oy + a[1]);
    let (bx, by) = (ox + b[0], oy + b[1]);
    let (vx, vy) = (bx - ax, by - ay);
    let len2 = vx * vx + vy * vy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * vx + (py - ay) * vy) / len2).clamp(0.0, 1.0)
    };
    let (dx, dy) = (px - (ax + t * vx), py - (ay + t * vy));
    dx * dx + dy * dy <= slop * slop
}

/// Topmost element under the point, or None. `ordered` is any iterator in paint
/// order (back → front, as `Scene::ordered()` yields); the LAST hit wins.
pub fn topmost<'a, I>(ordered: I, px: f64, py: f64) -> Option<&'a Element>
where
    I: IntoIterator<Item = &'a Element>,
{
    ordered
        .into_iter()
        .filter(|e| !e.isDeleted && e.kind != ElementType::Frame)
        .fold(None, |acc, e| if hits(e, px, py) { Some(e) } else { acc })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Element {
        Element {
            kind: ElementType::Rectangle,
            id: "r".into(),
            x,
            y,
            width: w,
            height: h,
            ..Default::default()
        }
    }

    #[test]
    fn bbox_hits() {
        let el = rect(0.0, 0.0, 100.0, 50.0);
        assert!(hits(&el, 50.0, 25.0));
        assert!(hits(&el, -2.0, 25.0)); // slop
        assert!(!hits(&el, 200.0, 25.0));
    }

    #[test]
    fn rotation_moves_hit_region() {
        let mut el = rect(0.0, 0.0, 100.0, 20.0);
        el.angle = std::f64::consts::FRAC_PI_2; // 90°: now a tall thin column
        assert!(hits(&el, 50.0 - 10.0, -30.0));
        // Far outside the rotated footprint.
        assert!(!hits(&el, -100.0, -100.0));
    }

    #[test]
    fn linear_hits_near_segment_not_bbox() {
        let el = Element {
            kind: ElementType::Arrow,
            id: "a".into(),
            x: 0.0,
            y: 0.0,
            points: Some(vec![[0.0, 0.0], [100.0, 0.0]]),
            ..Default::default()
        };
        assert!(hits(&el, 50.0, 2.0));
        assert!(!hits(&el, 50.0, 30.0), "bbox corner must not hit a line");
    }

    #[test]
    fn topmost_prefers_painted_later() {
        let bottom = Element { id: "bottom".into(), ..rect(0.0, 0.0, 100.0, 100.0) };
        let mut top = rect(40.0, 40.0, 100.0, 100.0);
        top.id = "top".into();
        top.index = Some("a1".into());
        let mut bottom = bottom;
        bottom.index = Some("a0".into());
        let ordered = [bottom, top]; // paint order
        let hit = topmost(ordered.iter(), 50.0, 50.0).unwrap();
        assert_eq!(hit.id, "top");
    }

    #[test]
    fn frames_dont_block_elements_behind_them() {
        let frame = Element {
            kind: ElementType::Frame,
            id: "f".into(),
            index: Some("a0".into()),
            ..rect(0.0, 0.0, 500.0, 500.0)
        };
        let mut inner = rect(240.0, 240.0, 20.0, 20.0);
        inner.id = "inner".into();
        inner.index = Some("a1".into());
        let ordered = [frame, inner];
        // Geometry hits the frame, but topmost resolution skips it.
        assert!(hits(&ordered[0], 250.0, 250.0));
        let hit = topmost(ordered.iter(), 250.0, 250.0).unwrap();
        assert_eq!(hit.id, "inner");
    }
}
