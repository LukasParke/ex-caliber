//! M5 text contract: reflow + label autofit through the public API.

use xc_core::edit;
use xc_core::element::{Element, ElementType};
use xc_core::scene::Scene;
use xc_core::text::TextEngine;

#[test]
fn mcp_style_text_patch_reflows_box() {
    let engine = TextEngine::bundled();
    let mut scene = Scene::new();
    scene
        .add(Element {
            kind: ElementType::Text,
            text: Some("hi".into()),
            fontSize: Some(20.0),
            lineHeight: Some(1.25),
            autoResize: Some(true),
            ..Default::default()
        })
        .unwrap();

    // Simulate update_elements: patch text, then reflow (what xc-mcp does).
    let mut el = scene.get(scene.ordered()[0].id.as_str()).unwrap().clone();
    el.text = Some("a much longer piece of text than before".into());
    engine.reflow(&mut el);
    scene.replace(el).unwrap();

    let after = scene.ordered()[0].clone();
    let (w, _) = engine.measure(
        "a much longer piece of text than before",
        "Excalifont",
        20.0,
        1.25,
    );
    assert!(
        (after.width - w).abs() < 2.0,
        "box must match measurement: {} vs {}",
        after.width,
        w
    );
}

#[test]
fn container_resize_refits_label_font() {
    let engine = TextEngine::bundled();
    let mut scene = Scene::new();
    scene
        .add(Element {
            kind: ElementType::Rectangle,
            id: "box".into(),
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 100.0,
            ..Default::default()
        })
        .unwrap();
    scene
        .add(Element {
            kind: ElementType::Text,
            id: "lbl".into(),
            containerId: Some("box".into()),
            text: Some("a reasonably long label text here".into()),
            originalText: Some("a reasonably long label text here".into()),
            fontSize: Some(20.0),
            lineHeight: Some(1.25),
            width: 250.0,
            height: 25.0,
            ..Default::default()
        })
        .unwrap();

    // Shrink the container hard; the label font must shrink with it.
    edit::resize_element(&mut scene, "box", 0.0, 0.0, 80.0, 100.0).unwrap();
    let label = scene.get("lbl").unwrap();
    let size = label.fontSize.unwrap_or(20.0);
    assert!(size < 20.0, "label font must shrink, got {size}");
    assert!(size >= 8.0, "font floor is 8px, got {size}");
    let measured = engine
        .measure(label.text.as_deref().unwrap(), "Excalifont", size, 1.25)
        .0;
    // Contract: font shrinks toward fit; at the 8px floor overflow is allowed
    // (excalidraw does the same).
    assert!(
        measured <= 72.0 || size <= 8.0,
        "measured {measured} at size {size}"
    );
}
