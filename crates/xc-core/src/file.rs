//! `.excalidraw` / `.excalidrawlib` load + save.
//!
//! Envelope verified against excalidraw source (`packages/excalidraw/data/json.ts`,
//! `packages/common/src/constants.ts`): `{ type: "excalidraw", version: 2, source,
//! elements, appState, files }`, pretty-printed with two-space indent; libraries use
//! `type: "excalidrawlib"`. Restore follows Excalidraw's `restore()` philosophy:
//! coerce bad values, backfill defaults, never lose fields we don't model.

use crate::element::{Element, ElementType};
use crate::scene::Scene;
use serde_json::{Value, json};

pub const SOURCE_TAG: &str = "excaliber";

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Json(String),
    Unsupported(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "io error: {e}"),
            LoadError::Json(e) => write!(f, "invalid json: {e}"),
            LoadError::Unsupported(kind) => write!(f, "unsupported document type: {kind}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Detect the document kind from raw JSON without full parsing.
pub fn detect_kind(doc: &Value) -> Option<&str> {
    doc.get("type").and_then(|t| t.as_str())
}

/// Load any supported document into a Scene.
///
/// Accepted inputs: `.excalidraw` envelope objects, bare `{elements: [...]}` objects,
/// top-level element arrays (Excalidraw clipboard format), and `.excalidrawlib`
/// libraries (their items' elements are flattened onto the canvas — same as dragging
/// library items out).
pub fn load_document(raw: &str) -> Result<Scene, LoadError> {
    let doc: Value = serde_json::from_str(raw).map_err(|e| LoadError::Json(e.to_string()))?;
    Ok(match detect_kind(&doc).unwrap_or("") {
        "excalidraw" => scene_from_envelope(&doc),
        "excalidrawlib" => scene_from_library(&doc),
        "" if doc.get("elements").is_some() => scene_from_envelope(&doc),
        "" if doc.is_array() => scene_from_elements(doc.as_array().unwrap().clone()),
        other => return Err(LoadError::Unsupported(other.to_string())),
    })
}

pub fn load_scene(path: &std::path::Path) -> Result<Scene, LoadError> {
    let raw = std::fs::read_to_string(path).map_err(LoadError::Io)?;
    load_document(&raw)
}

fn restore_element(mut el: Value) -> Option<Element> {
    let kind = el.get("type")?.as_str()?;
    let kind = match kind {
        "selection" => Some(ElementType::Selection),
        "rectangle" => Some(ElementType::Rectangle),
        "diamond" => Some(ElementType::Diamond),
        "ellipse" => Some(ElementType::Ellipse),
        "embeddable" => Some(ElementType::Embeddable),
        "iframe" => Some(ElementType::Iframe),
        "text" => Some(ElementType::Text),
        "line" => Some(ElementType::Line),
        "arrow" => Some(ElementType::Arrow),
        "freedraw" => Some(ElementType::Freedraw),
        "image" => Some(ElementType::Image),
        "frame" => Some(ElementType::Frame),
        "magicframe" => Some(ElementType::MagicFrame),
        _ => None,
    }?;

    // Coerce numeric-ish junk the way Excalidraw's restore does; anything still not
    // fitting our schema lands in `extras` untouched.
    for key in ["x", "y", "width", "height", "angle"] {
        coerce_number(&mut el, key);
    }
    for key in ["seed", "version", "versionNonce", "roughness", "opacity"] {
        coerce_number(&mut el, key);
    }

    let mut parsed: Element = serde_json::from_value(el).ok()?;
    parsed.kind = kind;
    // Bookkeeping backfill (restore semantics).
    if parsed.id.is_empty() {
        parsed.id = crate::idgen::new_id();
    }
    if parsed.seed == 0 {
        parsed.seed = crate::idgen::new_seed(None);
    }
    if parsed.version == 0 {
        parsed.version = 1;
    }
    if parsed.versionNonce == 0 {
        parsed.versionNonce = crate::idgen::new_seed(None);
    }
    if parsed.updated == 0 {
        parsed.updated = crate::time::now_ms();
    }
    Some(parsed)
}

fn coerce_number(el: &mut Value, key: &str) {
    if let Some(v) = el.get_mut(key)
        && !v.is_number()
        && !v.is_null()
        && let Ok(n) = v.as_str().unwrap_or("").parse::<f64>()
    {
        *v = json!(n);
    }
}

fn scene_from_elements(elements: Vec<Value>) -> Scene {
    let mut scene = Scene::new();
    for el in elements {
        if let Some(parsed) = restore_element(el) {
            let _ = scene.add_silent(parsed);
        }
    }
    scene
}

fn scene_from_envelope(doc: &Value) -> Scene {
    let mut scene = scene_from_elements(
        doc.get("elements")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default(),
    );
    scene.app_state = doc.get("appState").cloned().unwrap_or(Value::Null);
    scene.files = doc.get("files").cloned().unwrap_or(Value::Null);
    scene
}

fn scene_from_library(doc: &Value) -> Scene {
    // Library containers: items[] each carrying its own elements array.
    let items = doc
        .get("libraryItems")
        .or_else(|| doc.get("items"))
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();
    let mut all_elements = Vec::new();
    for item in items {
        if let Some(elements) = item.get("elements").and_then(|e| e.as_array()) {
            all_elements.extend(elements.clone());
        }
    }
    scene_from_elements(all_elements)
}

/// Serialize to the canonical pretty-printed `.excalidraw` form.
pub fn save_scene_to_string(scene: &Scene) -> String {
    let mut elements: Vec<&Element> = scene.elements_iter();
    elements.sort_by(|a, b| a.index.cmp(&b.index));
    let data = json!({
        "type": "excalidraw",
        "version": 2,
        "source": SOURCE_TAG,
        "elements": elements,
        "appState": scene.app_state,
        "files": scene.files,
    });
    serde_json::to_string_pretty(&data).expect("scene serializes")
}

pub fn save_scene(scene: &Scene, path: &std::path::Path) -> Result<(), LoadError> {
    let body = save_scene_to_string(scene);
    std::fs::write(path, body).map_err(LoadError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_synthetic_all_types_scene() {
        let raw = r##"{
          "type": "excalidraw", "version": 2, "source": "https://excalidraw.com",
          "elements": [
            {"type":"rectangle","id":"r1","x":0,"y":0,"width":100,"height":60,
             "seed":11,"version":3,"versionNonce":22,"index":"a0",
             "groupIds":["g1"],"frameId":null,"boundElements":[{"id":"a1","type":"arrow"}],
             "updated":1700000000000,"link":null,"locked":false,
             "customData":{"tag":"keep-me"},"brandNewField":{"x":1}},
            {"type":"text","id":"t1","x":5,"y":5,"width":40,"height":25,
             "fontSize":20,"fontFamily":1,"text":"hello","textAlign":"left",
             "verticalAlign":"top","containerId":null,"originalText":"hello",
             "autoResize":true,"lineHeight":1.25},
            {"type":"arrow","id":"a1","x":100,"y":30,"width":50,"height":0,
             "points":[[0,0],[50,0]],
             "startBinding":{"elementId":"r1","fixedPoint":[1.0,0.5],"mode":"orbit"},
             "endBinding":{"elementId":"old","focus":0.2,"gap":2.0},
             "elbowed":false},
            {"type":"freedraw","id":"f1","x":0,"y":0,"points":[[0,0],[3,4],[7,1]],
             "pressures":[0.5,0.6],"simulatePressure":true,
             "strokeOptions":{"variability":"constant","streamline":0.5}},
            {"type":"image","id":"i1","x":200,"y":0,"width":80,"height":80,
             "fileId":"img_1","status":"saved","scale":[1,-1],"crop":null},
            {"type":"frame","id":"fr1","x":0,"y":0,"width":300,"height":200,"name":"Box"}
          ],
          "appState": {"viewBackgroundColor":"#ffffff"},
          "files": {"img_1": {"mimeType":"image/png","id":"img_1","dataURL":"data:,","created":1}}
        }"##;
        let scene = load_document(raw).expect("loads");
        assert_eq!(scene.len(), 6);

        let out = save_scene_to_string(&scene);
        let reloaded = load_document(&out).expect("reloads");
        assert_eq!(reloaded.len(), 6);

        // Lossless details survive one round-trip.
        let r1 = reloaded.get("r1").unwrap();
        assert_eq!(r1.customData.as_ref().unwrap()["tag"], "keep-me");
        assert_eq!(r1.extras.get("brandNewField").unwrap()["x"], 1);
        assert_eq!(reloaded.app_state["viewBackgroundColor"], "#ffffff");

        let arrow = reloaded.get("a1").unwrap();
        let out_val = serde_json::to_value(arrow).unwrap();
        assert_eq!(
            out_val["startBinding"]["fixedPoint"],
            serde_json::json!([1.0, 0.5])
        );
        assert_eq!(out_val["endBinding"]["gap"], 2.0); // legacy preserved

        // Envelope is exactly what excalidraw.com reads.
        let env: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(env["type"], "excalidraw");
        assert_eq!(env["version"], 2);
        assert!(env["source"].is_string());
    }

    #[test]
    fn loads_library_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/fixture_library.excalidrawlib"
        );
        let raw = std::fs::read_to_string(path).unwrap();
        let scene = load_document(&raw).expect("library loads");
        // Fixture has empty items; loading must still succeed and produce a valid scene.
        assert_eq!(scene.len(), 0);
    }

    #[test]
    fn restores_minimal_legacy_shape_element() {
        // A hand-written 2019-era rectangle missing most modern fields.
        let raw = r#"{"type":"excalidraw","version":2,"source":"x","elements":[
            {"type":"rectangle","id":"old1","x":10,"y":10,"width":5,"height":5}
        ],"appState":{},"files":{}}"#;
        let scene = load_document(raw).unwrap();
        let el = scene.get("old1").unwrap();
        assert_eq!(el.opacity, 100);
        assert_eq!(el.strokeColor, "#1e1e1e");
        assert!(el.seed != 0);
        assert!(el.index.is_some());
    }
}
