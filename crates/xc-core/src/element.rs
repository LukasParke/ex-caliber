//! Typed mirror of the Excalidraw element model
//! (`packages/element/src/types.ts`, master @ 2026-08).
//!
//! Design: a single flat struct covering every element type's fields, all
//! `Option`/defaulted, plus an `extras` map that preserves any unknown fields so we
//! never drop data written by newer Excalidraw versions. `kind` selects which payload
//! fields are meaningful; accessors give per-type views. This mirrors how serialized
//! scenes actually look (flat JSON objects tagged by `"type"`) and keeps restore()
//! semantics trivial.

// Field names intentionally mirror Excalidraw's serialized JSON keys verbatim
// (`strokeColor`, `isDeleted`, `groupIds`, ...); they ARE the wire format.
#![allow(non_snake_case)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every element `type` discriminator in the Excalidraw scene format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ElementType {
    #[serde(rename = "selection")]
    Selection,
    #[default]
    #[serde(rename = "rectangle")]
    Rectangle,
    #[serde(rename = "diamond")]
    Diamond,
    #[serde(rename = "ellipse")]
    Ellipse,
    #[serde(rename = "embeddable")]
    Embeddable,
    #[serde(rename = "iframe")]
    Iframe,
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "line")]
    Line,
    #[serde(rename = "arrow")]
    Arrow,
    #[serde(rename = "freedraw")]
    Freedraw,
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "frame")]
    Frame,
    #[serde(rename = "magicframe")]
    MagicFrame,
}

impl ElementType {
    pub const fn as_str(self) -> &'static str {
        match self {
            ElementType::Selection => "selection",
            ElementType::Rectangle => "rectangle",
            ElementType::Diamond => "diamond",
            ElementType::Ellipse => "ellipse",
            ElementType::Embeddable => "embeddable",
            ElementType::Iframe => "iframe",
            ElementType::Text => "text",
            ElementType::Line => "line",
            ElementType::Arrow => "arrow",
            ElementType::Freedraw => "freedraw",
            ElementType::Image => "image",
            ElementType::Frame => "frame",
            ElementType::MagicFrame => "magicframe",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FillStyle {
    #[default]
    Hachure,
    CrossHatch,
    Solid,
    Zigzag,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StrokeStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

/// `roundness` object; `type` kept numeric because the enum values are part of the
/// file format (`LEGACY: 1`, `PROPORTIONAL_RADIUS: 2`, `ADAPTIVE_RADIUS: 3`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Roundness {
    #[serde(rename = "type")]
    pub kind: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// Arrow binding as written by current Excalidraw: a fixed point expressed as
/// ratios of the bound shape's width/height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixedPointBinding {
    #[serde(rename = "elementId")]
    pub element_id: String,
    #[serde(rename = "fixedPoint")]
    pub fixed_point: [f64; 2],
    pub mode: Option<BindMode>,
}

/// Legacy pre-fixed-point binding (`focus`/`gap`). Preserved verbatim so old files
/// round-trip losslessly; conversion to fixed-point happens on interactive edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyBinding {
    #[serde(rename = "elementId")]
    pub element_id: String,
    pub focus: f64,
    pub gap: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindMode {
    Inside,
    Orbit,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Binding {
    Fixed(FixedPointBinding),
    Legacy(LegacyBinding),
    Other(Value),
}

impl Binding {
    pub fn element_id(&self) -> &str {
        match self {
            Binding::Fixed(b) => &b.element_id,
            Binding::Legacy(b) => &b.element_id,
            Binding::Other(_) => "",
        }
    }
}

/// Arrowhead names incl. cardinality variants; stored as raw string for forward-compat.
pub type Arrowhead = String;

/// freedraw `strokeOptions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FreedrawStrokeOptions {
    #[serde(default = "default_variability")]
    pub variability: String,
    #[serde(default)]
    pub streamline: f64,
}

fn default_variability() -> String {
    "variable".to_string()
}

/// Element-local point `[x, y]`, relative to the element origin.
pub type LocalPoint = [f64; 2];

/// The complete element record. See module docs for the flat-struct rationale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    #[serde(rename = "type", default)]
    pub kind: ElementType,
    #[serde(default)]
    pub id: String,
    // ---- geometry ----
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    /// Radians.
    #[serde(default)]
    pub angle: f64,
    /// Points relative to `(x, y)` for line/arrow/freedraw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<LocalPoint>>,

    // ---- style ----
    #[serde(default = "default_stroke_color")]
    pub strokeColor: String,
    #[serde(default = "default_background_color")]
    pub backgroundColor: String,
    #[serde(default)]
    pub fillStyle: FillStyle,
    #[serde(default = "default_stroke_width")]
    pub strokeWidth: f64,
    #[serde(default)]
    pub strokeStyle: StrokeStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roundness: Option<Roundness>,
    #[serde(default = "default_roughness")]
    pub roughness: i64,
    #[serde(default = "default_opacity")]
    pub opacity: i64,

    // ---- collaboration / ordering bookkeeping ----
    /// Seed for deterministic rough-style geometry.
    #[serde(default)]
    pub seed: i64,
    #[serde(default = "default_version")]
    pub version: i64,
    #[serde(default)]
    pub versionNonce: i64,
    /// Fractional ordering key (rocicorp format); array order == index order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    #[serde(default)]
    pub isDeleted: bool,
    /// Deepest group first.
    #[serde(default)]
    pub groupIds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frameId: Option<String>,
    /// Elements bound TO this one (arrows, container labels).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundElements: Option<Vec<BoundElementRef>>,
    /// Epoch ms of last update.
    #[serde(default)]
    pub updated: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub customData: Option<Value>,

    // ---- text ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fontSize: Option<f64>,
    /// Numeric in the file format (1 = hand-drawn family, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fontFamily: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub textAlign: Option<TextAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verticalAlign: Option<VerticalAlign>,
    /// Container this label is bound to (text-in-shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containerId: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub originalText: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autoResize: Option<bool>,
    /// Unitless multiplier of fontSize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineHeight: Option<f64>,

    // ---- linear elements ----
    pub polygon: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startBinding: Option<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endBinding: Option<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startArrowhead: Option<Arrowhead>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endArrowhead: Option<Arrowhead>,

    // ---- elbow arrows ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elbowed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fixedSegments: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startIsSpecial: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endIsSpecial: Option<bool>,

    // ---- freedraw ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressures: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulatePressure: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strokeOptions: Option<FreedrawStrokeOptions>,

    // ---- image ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fileId: Option<String>,
    /// pending | saved | error
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Axis flip factors in <-1, 1>.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Value>,

    // ---- frame / embeddable / iframe ----
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Unknown fields from newer formats — preserved verbatim.
    #[serde(flatten)]
    pub extras: BTreeMap<String, Value>,
}

/// `Default` must agree with the serde field defaults — constructing via
/// `json!({})` deserialization guarantees it by construction (a hand-written
/// impl drifted once and produced invisible `strokeWidth: 0` arrows).
impl Default for Element {
    fn default() -> Self {
        serde_json::from_value(serde_json::json!({})).expect("empty object restores to defaults")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundElementRef {
    pub id: String,
    /// "arrow" | "text"
    pub r#type: String,
}

fn default_stroke_color() -> String {
    "#1e1e1e".to_string()
}
fn default_background_color() -> String {
    "transparent".to_string()
}
fn default_stroke_width() -> f64 {
    2.0
}
fn default_roughness() -> i64 {
    1
}
fn default_opacity() -> i64 {
    100
}
fn default_version() -> i64 {
    1
}

impl Element {
    pub fn is_linear(&self) -> bool {
        matches!(self.kind, ElementType::Line | ElementType::Arrow)
    }

    pub fn can_bind(&self) -> bool {
        matches!(
            self.kind,
            ElementType::Rectangle
                | ElementType::Diamond
                | ElementType::Ellipse
                | ElementType::Text
                | ElementType::Image
                | ElementType::Iframe
                | ElementType::Embeddable
                | ElementType::Frame
                | ElementType::MagicFrame
        )
    }

    pub fn is_deleted(&self) -> bool {
        self.isDeleted
    }

    /// Effective span of linear elements (width/height derive from points).
    pub fn effective_size(&self) -> (f64, f64) {
        if let Some(points) = &self.points {
            if points.is_empty() {
                return (self.width, self.height);
            }
            let (mut min_x, mut max_x) = (f64::MAX, f64::MIN);
            let (mut min_y, mut max_y) = (f64::MAX, f64::MIN);
            for [px, py] in points {
                min_x = min_x.min(*px);
                max_x = max_x.max(*px);
                min_y = min_y.min(*py);
                max_y = max_y.max(*py);
            }
            ((max_x - min_x).abs(), (max_y - min_y).abs())
        } else {
            (self.width, self.height)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_arrow_with_fixed_point_binding() {
        let json = r#"{
            "type": "arrow", "id": "a1", "x": 10, "y": 20, "width": 80, "height": 40,
            "points": [[0,0],[80,40]], "startBinding": null, "endBinding": null,
            "startArrowhead": null, "endArrowhead": "triangle", "elbowed": false
        }"#;
        let el: Element = serde_json::from_str(json).unwrap();
        assert_eq!(el.kind, ElementType::Arrow);
        assert_eq!(el.endArrowhead.as_deref(), Some("triangle"));
        assert_eq!(el.startBinding, None);
        assert_eq!(el.strokeWidth, 2.0); // defaulted
        assert_eq!(el.opacity, 100);
        assert!(el.index.is_none());
    }

    #[test]
    fn preserves_unknown_fields_and_legacy_bindings() {
        let json = r#"{
            "type": "line", "id": "l1", "x": 0, "y": 0, "points": [[0,0],[5,5]],
            "polygon": true, "someFutureField": {"nested": 42},
            "startBinding": {"elementId": "r1", "focus": 0.1, "gap": 4.0}
        }"#;
        let el: Element = serde_json::from_str(json).unwrap();
        assert_eq!(el.extras.get("someFutureField").unwrap()["nested"], 42);
        match el.startBinding.as_ref().unwrap() {
            Binding::Legacy(b) => {
                assert_eq!(b.element_id, "r1");
                assert!((b.focus - 0.1).abs() < 1e-9);
            }
            _ => panic!("legacy binding should parse as legacy"),
        }
        // Round-trip keeps both.
        let out = serde_json::to_value(&el).unwrap();
        assert_eq!(out["someFutureField"]["nested"], 42);
        assert_eq!(out["startBinding"]["focus"], 0.1);
    }


    #[test]
    fn fixed_point_binding_round_trips() {
        let json = r#"{
            "type": "arrow", "id": "a2", "x": 0, "y": 0,
            "points": [[0,0],[10,10]],
            "endBinding": {"elementId": "e1", "fixedPoint": [0.25, 0.75], "mode": "orbit"}
        }"#;
        let el: Element = serde_json::from_str(json).unwrap();
        match el.endBinding.as_ref().unwrap() {
            Binding::Fixed(b) => {
                assert_eq!(b.fixed_point, [0.25, 0.75]);
                assert_eq!(b.mode, Some(BindMode::Orbit));
            }
            _ => panic!("expected fixed binding"),
        }
    }
}

#[cfg(test)]
mod default_tests {
    use super::*;

    #[test]
    fn default_matches_serde_defaults() {
        let d = Element::default();
        assert_eq!(d.strokeWidth, 2.0);
        assert_eq!(d.opacity, 100);
        assert_eq!(d.strokeColor, "#1e1e1e");
        assert_eq!(d.roughness, 1);
        assert_eq!(d.kind, ElementType::Rectangle);
    }
}
