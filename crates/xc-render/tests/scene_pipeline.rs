//! Integration: every live element of a real scene fixture renders without error,
//! and the pipeline stays deterministic across runs.

use xc_core::file;
use xc_render::geometry::render_element;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../xc-core/tests/fixtures");

#[test]
fn renders_whole_corpus_without_error() {
    let mut checked = 0;
    for entry in std::fs::read_dir(FIXTURES).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("excalidraw") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).unwrap();
        let scene = file::load_document(&raw).expect("loads");
        for el in scene.ordered() {
            let ops = render_element(el).expect("renders");
            let glyph_layer = matches!(
                el.kind,
                xc_core::element::ElementType::Text | xc_core::element::ElementType::Image
            );
            if !el.isDeleted && !glyph_layer {
                assert!(!ops.is_empty(), "{} produced no ops in {}", el.id, path.display());
            }
            checked += 1;
        }
    }
    assert!(checked > 0, "corpus is empty");
}

#[test]
fn corpus_render_is_deterministic() {
    let path = std::path::Path::new(FIXTURES).join("all_types.excalidraw");
    let raw = std::fs::read_to_string(&path).unwrap();
    let scene = file::load_document(&raw).unwrap();
    let first: Vec<String> = scene
        .ordered()
        .iter()
        .map(|el| format!("{:?}", render_element(el).unwrap()))
        .collect();
    let second: Vec<String> = scene
        .ordered()
        .iter()
        .map(|el| format!("{:?}", render_element(el).unwrap()))
        .collect();
    assert_eq!(first, second);
}
