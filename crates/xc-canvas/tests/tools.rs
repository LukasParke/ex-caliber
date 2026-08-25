//! Interaction contract tests: the ToolState state machine against a real Scene.

use xc_canvas::tools::{Tool, ToolState};
use xc_core::element::{Element, ElementType};
use xc_core::scene::Scene;

fn scene_with_two_boxes() -> Scene {
    let mut scene = Scene::new();
    scene
        .add(Element {
            kind: ElementType::Rectangle,
            id: "left".into(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
            ..Default::default()
        })
        .unwrap();
    scene
        .add(Element {
            kind: ElementType::Rectangle,
            id: "right".into(),
            x: 300.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
            ..Default::default()
        })
        .unwrap();
    scene
}

#[test]
fn drag_creates_rectangle_and_returns_to_select() {
    let mut scene = scene_with_two_boxes();
    let mut ts = ToolState::default();
    ts.set_tool(Tool::Rectangle);

    ts.pointer_down(&mut scene, 0.0, 200.0, false);
    ts.pointer_move(&mut scene, 150.0, 280.0, false);
    ts.pointer_up(&mut scene, 150.0, 280.0);

    assert_eq!(scene.len(), 3);
    let ordered = scene.ordered();
    let created = ordered.last().unwrap();
    assert_eq!(created.kind, ElementType::Rectangle);
    assert_eq!((created.x, created.y, created.width, created.height), (0.0, 200.0, 150.0, 80.0));
    assert_eq!(ts.tool, Tool::Select, "excalidraw returns to select after draw");
    assert!(ts.is_selected(created.id.as_str()));
}

#[test]
fn click_without_drag_drops_default_size() {
    let mut scene = Scene::new();
    let mut ts = ToolState::default();
    ts.set_tool(Tool::Ellipse);
    ts.pointer_down(&mut scene, 50.0, 50.0, false);
    ts.pointer_up(&mut scene, 50.0, 50.0);

    let ordered = scene.ordered();
    let el = ordered.last().unwrap();
    assert_eq!(el.kind, ElementType::Ellipse);
    assert_eq!((el.width, el.height), (120.0, 80.0));
}

#[test]
fn select_and_move_is_one_undo_step() {
    let mut scene = scene_with_two_boxes();
    let mut ts = ToolState::default();

    ts.pointer_down(&mut scene, 50.0, 30.0, false); // hits "left"
    ts.pointer_move(&mut scene, 80.0, 60.0, false);
    ts.pointer_move(&mut scene, 120.0, 90.0, false);
    ts.pointer_up(&mut scene, 120.0, 90.0);

    let left = scene.get("left").unwrap();
    assert_eq!((left.x, left.y), (70.0, 60.0), "element follows cursor delta");
    assert_eq!(scene.get("right").unwrap().x, 300.0, "unselected stays");

    assert!(scene.undo());
    assert_eq!(scene.get("left").unwrap().x, 0.0);
    assert!(scene.redo());
    assert_eq!(scene.get("left").unwrap().x, 70.0);
}

#[test]
fn moving_box_drags_its_bound_arrow() {
    let mut scene = Scene::new();
    scene
        .add(Element {
            kind: ElementType::Rectangle,
            id: "a".into(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
            ..Default::default()
        })
        .unwrap();
    scene
        .add(Element {
            kind: ElementType::Rectangle,
            id: "b".into(),
            x: 300.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
            ..Default::default()
        })
        .unwrap();
    let mut ts = ToolState::default();
    let arrow_id = ts.connect_ids(&mut scene, "a", "b").unwrap();

    // Select "a" (its center) and drag.
    ts.pointer_down(&mut scene, 50.0, 30.0, false);
    ts.pointer_move(&mut scene, 90.0, 30.0, false);
    ts.pointer_up(&mut scene, 90.0, 30.0);

    let arrow = scene.get(&arrow_id).unwrap();
    let pts = arrow.points.as_ref().unwrap();
    // World coordinates (points are relative to the arrow origin, which moves too).
    let start_world = [arrow.x + pts[0][0], arrow.y + pts[0][1]];
    let end_world = [arrow.x + pts[1][0], arrow.y + pts[1][1]];
    assert_eq!(start_world, [140.0, 30.0], "start follows dragged a");
    assert_eq!(end_world, [300.0, 30.0], "b end stays anchored");
}

#[test]
fn marquee_selects_intersecting_elements() {
    let mut scene = scene_with_two_boxes();
    let mut ts = ToolState::default();

    ts.pointer_down(&mut scene, -50.0, -50.0, false);
    ts.pointer_move(&mut scene, 50.0, 50.0, false); // only "left" inside
    assert!(ts.is_selected("left"));
    assert!(!ts.is_selected("right"));
    ts.pointer_move(&mut scene, 500.0, 500.0, false); // both inside
    ts.pointer_up(&mut scene, 500.0, 500.0);

    assert_eq!(ts.selection.len(), 2);
    assert_eq!(ts.ghost().marquee, None, "marquee clears on release");
}

#[test]
fn shift_click_toggles_selection() {
    let mut scene = scene_with_two_boxes();
    let mut ts = ToolState::default();

    ts.pointer_down(&mut scene, 50.0, 30.0, false);
    ts.pointer_up(&mut scene, 50.0, 30.0);
    assert_eq!(ts.selection.len(), 1);

    ts.pointer_down(&mut scene, 350.0, 30.0, true); // shift-add "right"
    ts.pointer_up(&mut scene, 350.0, 30.0);
    assert_eq!(ts.selection.len(), 2);

    ts.pointer_down(&mut scene, 350.0, 30.0, true); // shift again removes
    ts.pointer_up(&mut scene, 350.0, 30.0);
    assert_eq!(ts.selection.len(), 1);
}

#[test]
fn delete_and_duplicate_selection() {
    let mut scene = scene_with_two_boxes();
    let mut ts = ToolState::default();

    ts.pointer_down(&mut scene, 50.0, 30.0, false);
    ts.pointer_up(&mut scene, 50.0, 30.0);
    ts.duplicate_selection(&mut scene);
    assert_eq!(scene.ordered().len(), 3);

    ts.delete_selection(&mut scene);
    assert_eq!(scene.ordered().len(), 2, "tombstones stay, live count drops");
    assert!(ts.selection.is_empty());
}

#[test]
fn freedraw_accumulates_points_and_commits() {
    let mut scene = Scene::new();
    let mut ts = ToolState::default();
    ts.set_tool(Tool::Freedraw);

    ts.pointer_down(&mut scene, 0.0, 0.0, false);
    ts.pointer_move(&mut scene, 10.0, 5.0, false);
    ts.pointer_move(&mut scene, 20.0, 12.0, false);
    ts.pointer_up(&mut scene, 20.0, 12.0);

    assert_eq!(scene.len(), 1);
    let ordered = scene.ordered();
    let sketch = ordered.last().unwrap();
    assert_eq!(sketch.kind, ElementType::Freedraw);
    assert_eq!(sketch.points.as_ref().unwrap().len(), 3);
}

#[test]
fn tool_keys_map_like_excalidraw() {
    assert_eq!(Tool::from_key("r"), Some(Tool::Rectangle));
    assert_eq!(Tool::from_key("d"), Some(Tool::Diamond));
    assert_eq!(Tool::from_key("o"), Some(Tool::Ellipse));
    assert_eq!(Tool::from_key("a"), Some(Tool::Arrow));
    assert_eq!(Tool::from_key("x"), Some(Tool::Freedraw));
    assert_eq!(Tool::from_key("t"), Some(Tool::Text));
    assert_eq!(Tool::from_key("q"), None);
}

#[test]
fn arrow_drag_snaps_with_shift() {
    let mut scene = Scene::new();
    let mut ts = ToolState::default();
    ts.set_tool(Tool::Arrow);

    ts.pointer_down(&mut scene, 0.0, 0.0, false);
    ts.pointer_move(&mut scene, 90.0, 10.0, true); // near-horizontal → snaps to 0°
    let pts = ts.ghost().element.unwrap().points.unwrap();
    assert_eq!(pts[1][1], 0.0, "shift snaps to 45° multiples");
}
