//! Editing-operation contract tests: binding recompute, labels, groups,
//! duplication, alignment — all through the public `edit` API with undo checks.

use xc_core::edit::{self, AlignMode};
use xc_core::element::{Binding, Element, ElementType, FixedPointBinding};
use xc_core::scene::Scene;

fn rect(id: &str, x: f64, y: f64, w: f64, h: f64) -> Element {
    Element {
        kind: ElementType::Rectangle,
        id: id.into(),
        x,
        y,
        width: w,
        height: h,
        ..Default::default()
    }
}

fn arrow(id: &str, from: &str, to: &str) -> Element {
    // Anchored as `connect` would: a right-center → b left-center.
    let _ = (from, to);
    Element {
        kind: ElementType::Arrow,
        id: id.into(),
        points: Some(vec![[100.0, 30.0], [300.0, 30.0]]),
        startBinding: Some(Binding::Fixed(FixedPointBinding {
            element_id: from.into(),
            fixed_point: [1.0, 0.5],
            mode: None,
        })),
        endBinding: Some(Binding::Fixed(FixedPointBinding {
            element_id: to.into(),
            fixed_point: [0.0, 0.5],
            mode: None,
        })),
        endArrowhead: Some("triangle".into()),
        ..Default::default()
    }
}

fn label(id: &str, container: &str) -> Element {
    Element {
        kind: ElementType::Text,
        id: id.into(),
        containerId: Some(container.into()),
        width: 40.0,
        height: 24.0,
        text: Some("hi".into()),
        originalText: Some("hi".into()),
        ..Default::default()
    }
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn moving_shape_reanchors_bound_arrow() {
    let mut scene = Scene::new();
    let a = scene.add(rect("a", 0.0, 0.0, 100.0, 60.0)).unwrap();
    let b = scene.add(rect("b", 300.0, 0.0, 100.0, 60.0)).unwrap();
    let arrow_id = scene.add(arrow("ar", "a", "b")).unwrap();

    // Start anchor sits at a's right-center: (100, 30).
    let before = scene.get(&arrow_id).unwrap().clone();
    assert_eq!(before.points.as_ref().unwrap()[0], [100.0, 30.0]);

    edit::move_elements(&mut scene, std::slice::from_ref(&a), 50.0, 25.0).unwrap();

    let arrow_after = scene.get(&arrow_id).unwrap();
    let pts = arrow_after.points.as_ref().unwrap();
    assert_eq!(pts[0], [150.0, 55.0], "start follows moved a");
    assert_eq!(pts[1], [300.0, 30.0], "unmoved end stays");
    assert_eq!(scene.get(&b).unwrap().x, 300.0);
}

#[test]
fn resizing_shape_keeps_fixed_point_ratio() {
    let mut scene = Scene::new();
    scene.add(rect("a", 0.0, 0.0, 100.0, 60.0)).unwrap();
    scene.add(rect("b", 300.0, 0.0, 100.0, 60.0)).unwrap();
    let arrow_id = scene.add(arrow("ar", "a", "b")).unwrap();

    // a grows to 200x120: fixedPoint [1, 0.5] → anchor at (200, 60).
    edit::resize_element(&mut scene, "a", 0.0, 0.0, 200.0, 120.0).unwrap();
    let pts = scene.get(&arrow_id).unwrap().points.as_ref().unwrap();
    assert_eq!(pts[0], [200.0, 60.0]);
}

#[test]
fn container_labels_recenter_on_move() {
    let mut scene = Scene::new();
    scene.add(rect("box", 0.0, 0.0, 200.0, 100.0)).unwrap();
    scene.add(label("lbl", "box")).unwrap();

    edit::move_elements(&mut scene, &["box".to_string()], 100.0, 50.0).unwrap();
    let lbl = scene.get("lbl").unwrap();
    assert_eq!(lbl.x, 100.0 + (200.0 - 40.0) / 2.0);
    assert_eq!(lbl.y, 50.0 + (100.0 - 24.0) / 2.0);
}

#[test]
fn move_is_one_undo_step_and_reversible() {
    let mut scene = Scene::new();
    scene.add(rect("a", 0.0, 0.0, 100.0, 60.0)).unwrap();
    scene.add(rect("b", 300.0, 0.0, 100.0, 60.0)).unwrap();
    let arrow_id = scene.add(arrow("ar", "a", "b")).unwrap();
    let before = scene.get(&arrow_id).unwrap().clone();

    edit::move_elements(&mut scene, &["a".to_string()], 40.0, 0.0).unwrap();
    assert_ne!(scene.get(&arrow_id).unwrap(), &before);

    assert!(scene.undo());
    assert_eq!(scene.get(&arrow_id).unwrap(), &before, "undo restores arrow too");
    assert_eq!(scene.get("a").unwrap().x, 0.0);
}

#[test]
fn duplicate_gets_fresh_identity() {
    let mut scene = Scene::new();
    let a = scene.add(rect("a", 0.0, 0.0, 100.0, 60.0)).unwrap();
    let new_ids = edit::duplicate(&mut scene, &[a]).unwrap();
    let copy = scene.get(&new_ids[0]).unwrap();
    assert_ne!(copy.id, "a");
    assert_ne!(copy.seed, scene.get("a").unwrap().seed);
    assert_eq!((copy.x, copy.y, copy.width, copy.height), (0.0, 0.0, 100.0, 60.0));
    assert!(copy.boundElements.is_none());
}

#[test]
fn group_and_ungroup_round_trip() {
    let mut scene = Scene::new();
    scene.add(rect("a", 0.0, 0.0, 10.0, 10.0)).unwrap();
    scene.add(rect("b", 20.0, 0.0, 10.0, 10.0)).unwrap();

    let gid = edit::group(&mut scene, &ids(&["a", "b"])).unwrap();
    assert_eq!(scene.get("a").unwrap().groupIds, vec![gid.clone()]);
    assert_eq!(edit::common_group(&scene, &ids(&["a", "b"])).as_deref(), Some(gid.as_str()));

    edit::ungroup(&mut scene, &ids(&["a", "b"])).unwrap();
    assert!(scene.get("a").unwrap().groupIds.is_empty());
}

#[test]
fn align_lefts() {
    let mut scene = Scene::new();
    scene.add(rect("a", 10.0, 0.0, 20.0, 10.0)).unwrap();
    scene.add(rect("b", 60.0, 0.0, 30.0, 10.0)).unwrap();
    edit::align(&mut scene, &ids(&["a", "b"]), AlignMode::Left).unwrap();
    assert_eq!(scene.get("a").unwrap().x, 10.0);
    assert_eq!(scene.get("b").unwrap().x, 10.0);
}

#[test]
fn moving_both_ends_translates_whole_arrow() {
    let mut scene = Scene::new();
    scene.add(rect("a", 0.0, 0.0, 100.0, 60.0)).unwrap();
    scene.add(rect("b", 300.0, 0.0, 100.0, 60.0)).unwrap();
    let arrow_id = scene.add(arrow("ar", "a", "b")).unwrap();

    edit::move_elements(&mut scene, &ids(&["a", "b"]), 10.0, 5.0).unwrap();
    let pts = scene.get(&arrow_id).unwrap().points.as_ref().unwrap();
    assert_eq!(pts[0], [110.0, 35.0]);
    assert_eq!(pts[1], [310.0, 35.0]);
}
