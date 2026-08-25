//! Elbow-arrow routing: orthogonal paths between anchor points.
//!
//! v1 contract (documented divergence): a 3-segment orthogonal route through the
//! dominant-axis midpoint, matching excalidraw's default look for well-separated
//! shapes. Obstacle avoidance and user-pinned `fixedSegments` *editing* are out
//! of scope; arrows WITH fixedSegments keep their stored points verbatim.

/// Route orthogonally from `start` to `end` (world coords).
/// Consecutive points always share an axis; the path starts at start, ends at end.
pub fn route_elbow(start: [f64; 2], end: [f64; 2]) -> Vec<[f64; 2]> {
    let [sx, sy] = start;
    let [ex, ey] = end;
    if sx == ex || sy == ey {
        // Already collinear on an axis: straight line.
        return vec![start, end];
    }
    let dx = (ex - sx).abs();
    let dy = (ey - sy).abs();
    if dx >= dy {
        // Leave horizontally: bend at the horizontal midpoint.
        let mid_x = (sx + ex) / 2.0;
        vec![start, [mid_x, sy], [mid_x, ey], end]
    } else {
        // Leave vertically: bend at the vertical midpoint.
        let mid_y = (sy + ey) / 2.0;
        vec![start, [sx, mid_y], [ex, mid_y], end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_orthogonal(path: &[[f64; 2]]) {
        for w in path.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(
                (a[0] - b[0]).abs() < 1e-9 || (a[1] - b[1]).abs() < 1e-9,
                "segment {a:?}->{b:?} is not axis-aligned"
            );
        }
    }

    #[test]
    fn horizontal_dominant_routes_through_mid_x() {
        let path = route_elbow([0.0, 0.0], [300.0, 120.0]);
        assert_orthogonal(&path);
        assert_eq!(path.first(), Some(&[0.0, 0.0]));
        assert_eq!(path.last(), Some(&[300.0, 120.0]));
        assert_eq!(path.len(), 4);
        assert_eq!(path[1][0], 150.0);
    }

    #[test]
    fn vertical_dominant_routes_through_mid_y() {
        let path = route_elbow([0.0, 0.0], [60.0, 300.0]);
        assert_orthogonal(&path);
        assert_eq!(path[1][1], 150.0);
    }

    #[test]
    fn collinear_inputs_stay_straight() {
        assert_eq!(route_elbow([0.0, 0.0], [100.0, 0.0]), vec![[0.0, 0.0], [100.0, 0.0]]);
        assert_eq!(route_elbow([0.0, 0.0], [0.0, 100.0]), vec![[0.0, 0.0], [0.0, 100.0]]);
    }

    #[test]
    fn negative_directions_route() {
        let path = route_elbow([500.0, 100.0], [100.0, 400.0]);
        assert_orthogonal(&path);
        assert_eq!(path.first(), Some(&[500.0, 100.0]));
        assert_eq!(path.last(), Some(&[100.0, 400.0]));
    }
}
