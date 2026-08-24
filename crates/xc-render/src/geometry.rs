//! Element → deterministic rough-style draw ops (headless, toolkit-free).
//!
//! Every op derives from the element's stored `seed`, so the same scene renders
//! identically everywhere — the property the whole test strategy leans on.
//! Visual parity with Excalidraw's renderer is structural, not pixel-level
//! (plan §1 non-goals): roughness mapping is approximated.

use lyon::math::{point, Point};
use lyon::path::Path as LyonPath;
use roughr::core::{FillStyle, Op, OpSetType, Options};
use roughr::Srgba;
use roughr::generator::Generator;

use xc_core::element::{Element, ElementType, Roundness, StrokeStyle};

/// One renderable primitive in element-local coordinates (origin = element x,y).
#[derive(Debug, Clone)]
pub enum DrawOp {
    /// Filled outline (solid fill).
    Fill { path: LyonPath, color: Srgba },
    /// Stroked polyline/curve.
    Stroke {
        path: LyonPath,
        width: f32,
        color: Srgba,
        dash: Option<&'static [f32]>,
    },
}

#[derive(Debug, Clone)]
pub struct RenderElementError(pub String);

impl std::fmt::Display for RenderElementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "render error: {}", self.0)
    }
}
impl std::error::Error for RenderElementError {}

fn err<T>(msg: impl Into<String>) -> Result<T, RenderElementError> {
    Err(RenderElementError(msg.into()))
}

/// Excalidraw roughness (0 architect, 1 artist, 2 cartoonist) → rough.js options.
fn rough_options(el: &Element) -> Options {
    let r = el.roughness.clamp(0, 2);
    let seed = if el.seed == 0 { 1u64 } else { el.seed.unsigned_abs() };
    Options {
        seed: Some(seed),
        roughness: Some([0.5, 1.0, 2.0][r as usize]),
        bowing: Some([0.5, 1.0, 3.0][r as usize]),
        stroke: Some(parse_color(&el.strokeColor)),
        stroke_width: Some(el.strokeWidth as f32),
        fill: (el.backgroundColor != "transparent").then(|| parse_color(&el.backgroundColor)),
        fill_style: Some(match el.fillStyle {
            xc_core::element::FillStyle::Hachure => FillStyle::Hachure,
            xc_core::element::FillStyle::CrossHatch => FillStyle::CrossHatch,
            xc_core::element::FillStyle::Solid => FillStyle::Solid,
            xc_core::element::FillStyle::Zigzag => FillStyle::ZigZag,
        }),
        hachure_gap: Some(4.0 * el.strokeWidth as f32),
        hachure_angle: Some(-41.0),
        disable_multi_stroke: Some(true),
        ..Default::default()
    }
}

/// "#rrggbb", "#rrggbbaa" or a handful of named colors Excalidraw writes.
pub fn parse_color(s: &str) -> Srgba {
    if let Some(hex) = s.strip_prefix('#') {
        let bytes = hex.as_bytes();
        let hexval = |i: usize| -> f32 {
            u8::from_str_radix(&hex[i..i + 2], 16).map(|v| v as f32 / 255.0).unwrap_or(0.0)
        };
        match bytes.len() {
            6 => Srgba::new(hexval(0), hexval(2), hexval(4), 1.0),
            8 => Srgba::new(hexval(0), hexval(2), hexval(4), hexval(6)),
            _ => Srgba::new(0.0, 0.0, 0.0, 1.0),
        }
    } else {
        match s {
            "transparent" => Srgba::new(0.0, 0.0, 0.0, 0.0),
            "white" => Srgba::new(1.0, 1.0, 1.0, 1.0),
            "black" => Srgba::new(0.1, 0.1, 0.1, 1.0),
            "red" => Srgba::new(0.86, 0.19, 0.19, 1.0),
            "green" => Srgba::new(0.11, 0.72, 0.35, 1.0),
            "blue" => Srgba::new(0.22, 0.47, 0.85, 1.0),
            _ => Srgba::new(0.1, 0.1, 0.1, 1.0),
        }
    }
}

fn dash_for(el: &Element) -> Option<&'static [f32]> {
    match el.strokeStyle {
        StrokeStyle::Solid => None,
        StrokeStyle::Dashed => Some(&[8.0, 6.0]),
        StrokeStyle::Dotted => Some(&[1.5, 4.0]),
    }
}

/// Render one element to draw ops in element-local space (rotation NOT applied —
/// the adapter transforms around the element center per frame).
pub fn render_element(el: &Element) -> Result<Vec<DrawOp>, RenderElementError> {
    let (w, h) = el.effective_size();
    if el.isDeleted {
        return Ok(vec![]);
    }
    let generator = Generator::default();
    let opts = rough_options(el);
    let mut ops = Vec::new();

    match el.kind {
        ElementType::Rectangle if el.roundness.is_some() => {
            return render_rounded_rect(el);
        }
        ElementType::Rectangle => {
            let drawable = generator.rectangle(0.0, 0.0, w as f32, h as f32, &Some(opts.clone()));
            push_drawable(&mut ops, drawable, &opts, dash_for(el));
        }
        ElementType::Ellipse => {
            let drawable =
                generator.ellipse(w as f32 / 2.0, h as f32 / 2.0, w as f32, h as f32, &Some(opts.clone()));
            push_drawable(&mut ops, drawable, &opts, dash_for(el));
        }
        ElementType::Diamond => {
            let pts = [
                point(w as f32 / 2.0, 0.0),
                point(w as f32, h as f32 / 2.0),
                point(w as f32 / 2.0, h as f32),
                point(0.0, h as f32 / 2.0),
            ];
            let drawable = generator.polygon(&pts, &Some(opts.clone()));
            push_drawable(&mut ops, drawable, &opts, dash_for(el));
        }
        ElementType::Line | ElementType::Arrow => {
            let pts = el.points.as_deref().unwrap_or(&[]);
            if pts.len() < 2 {
                return err("linear element with fewer than 2 points");
            }
            let f32pts: Vec<Point> = pts.iter().map(|[x, y]| point(*x as f32, *y as f32)).collect();
            let drawable = generator.linear_path(&f32pts, false, &Some(opts.clone()));
            push_drawable(&mut ops, drawable, &opts, dash_for(el));
            if el.kind == ElementType::Arrow {
                if el.endArrowhead.is_some() {
                    ops.push(arrowhead(pts, false, &parse_color(&el.strokeColor)));
                }
                if el.startArrowhead.is_some() {
                    ops.push(arrowhead(pts, true, &parse_color(&el.strokeColor)));
                }
            }
        }
        ElementType::Freedraw => {
            // Freedraw strokes are the user's actual input — never roughened.
            let pts = el.points.as_deref().unwrap_or(&[]);
            if pts.len() < 2 {
                return Ok(vec![]);
            }
            let mut b = LyonPath::builder();
            b.begin(point(pts[0][0] as f32, pts[0][1] as f32));
            for [x, y] in &pts[1..] {
                b.line_to(point(*x as f32, *y as f32));
            }
            b.end(false);
            let path = b.build();
            ops.push(DrawOp::Stroke {
                path,
                width: el.strokeWidth as f32,
                color: parse_color(&el.strokeColor),
                dash: None,
            });
        }
        ElementType::Frame => {
            // Frame chrome: dashed rectangle, transparent fill.
            let frame_opts = Options {
                fill: None,
                stroke: Some(Srgba::new(0.73, 0.73, 0.73, 1.0)),
                stroke_line_dash: Some(vec![6.0, 6.0]),
                ..opts.clone()
            };
            let drawable = generator.rectangle(0.0, 0.0, w as f32, h as f32, &Some(frame_opts));
            push_drawable(&mut ops, drawable, &opts, dash_for(el));
        }
        // Text (glyph layer), Image (bitmap layer) render outside geometry.
        ElementType::Text | ElementType::Image => return Ok(vec![]),
        ElementType::Selection
        | ElementType::Embeddable
        | ElementType::Iframe
        | ElementType::MagicFrame => {
            let drawable = generator.rectangle(0.0, 0.0, w as f32, h as f32, &Some(opts.clone()));
            push_drawable(&mut ops, drawable, &opts, dash_for(el));
        }
    }
    Ok(ops)
}

/// Rounded-rectangle drawable (used when `roundness` is present).
pub fn render_rounded_rect(el: &Element) -> Result<Vec<DrawOp>, RenderElementError> {
    let (w, h) = el.effective_size();
    let Some(Roundness { kind, value }) = el.roundness else {
        return err("no roundness");
    };
    let radius = match kind {
        3 => value.unwrap_or(8.0).min(w.min(h) / 2.0),
        _ => w.min(h) * 0.25,
    };
    let generator = Generator::default();
    let opts = rough_options(el);
    let d = format!(
        "M {r} 0 L {wr} 0 A {r} {r} 0 0 1 {w} {r} L {w} {hr} A {r} {r} 0 0 1 {wr} {h} \
         L {r} {h} A {r} {r} 0 0 1 0 {hr} L 0 {r} A {r} {r} 0 0 1 {r} 0 Z",
        r = fmt_f(radius),
        wr = fmt_f(w - radius),
        hr = fmt_f(h - radius),
        w = fmt_f(w),
        h = fmt_f(h),
    );
    let drawable = generator.path(d, &Some(opts.clone()));
    let mut ops = Vec::new();
    push_drawable(&mut ops, drawable, &opts, dash_for(el));
    Ok(ops)
}

fn fmt_f(v: f64) -> String {
    format!("{v:.2}")
}

/// Convert roughr opsets into draw ops. Stroke opsets → `Stroke`; solid fills →
/// `Fill`; hachure/zigzag fills arrive as `FillSketch` opsets (thin strokes in the
/// fill color).
fn push_drawable(
    ops: &mut Vec<DrawOp>,
    drawable: roughr::core::Drawable<f32>,
    opts: &Options,
    dash: Option<&'static [f32]>,
) {
    for set in &drawable.sets {
        let Some(path) = opset_to_path(&set.ops) else { continue };
        match set.op_set_type {
            OpSetType::Path => ops.push(DrawOp::Stroke {
                path,
                width: opts.stroke_width.unwrap_or(1.0),
                color: opts.stroke.unwrap_or_else(|| Srgba::new(0.0, 0.0, 0.0, 1.0)),
                dash,
            }),
            OpSetType::FillPath => {
                let Some(color) = opts.fill else { continue };
                ops.push(DrawOp::Fill { path, color });
            }
            OpSetType::FillSketch => {
                let Some(color) = opts.fill else { continue };
                ops.push(DrawOp::Stroke {
                    path,
                    width: opts.fill_weight.unwrap_or(1.0).abs().max(1.0),
                    color,
                    dash: None,
                });
            }
        }
    }
}

/// roughr ops (Move/LineTo/BCurveTo) → lyon path.
fn opset_to_path(ops: &[Op<f32>]) -> Option<LyonPath> {
    let mut b = LyonPath::builder();
    let mut started = false;
    for op in ops {
        match op.op {
            roughr::core::OpType::Move => {
                let (x, y) = (op.data[0], op.data[1]);
                if started {
                    b.end(false);
                }
                b.begin(point(x, y));
                started = true;
            }
            roughr::core::OpType::LineTo => {
                if !started {
                    b.begin(point(op.data[0], op.data[1]));
                    started = true;
                    continue;
                }
                b.line_to(point(op.data[0], op.data[1]));
            }
            roughr::core::OpType::BCurveTo => {
                if !started {
                    continue;
                }
                // data: [c1x, c1y, c2x, c2y, x, y]
                b.cubic_bezier_to(point(op.data[0], op.data[1]), point(op.data[2], op.data[3]), point(op.data[4], op.data[5]));
            }
        }
    }
    if started {
        b.end(false);
        Some(b.build())
    } else {
        None
    }
}

/// Crisp (non-roughened) arrowhead triangle at a linear element's end.
fn arrowhead(points: &[[f64; 2]], at_start: bool, color: &Srgba) -> DrawOp {
    let n = points.len();
    let (tip, prev) = if at_start {
        (points[0], points[1])
    } else {
        (points[n - 1], points[n - 2])
    };
    let angle = (tip[1] - prev[1]).atan2(tip[0] - prev[0]);
    let len = 12.0_f64;
    let spread = 0.42_f64;
    let p1 = [
        tip[0] - len * (angle - spread).cos(),
        tip[1] - len * (angle - spread).sin(),
    ];
    let p2 = [
        tip[0] - len * (angle + spread).cos(),
        tip[1] - len * (angle + spread).sin(),
    ];
    let mut b = LyonPath::builder();
    b.begin(point(tip[0] as f32, tip[1] as f32));
    b.line_to(point(p1[0] as f32, p1[1] as f32));
    b.line_to(point(p2[0] as f32, p2[1] as f32));
    b.end(true);
    DrawOp::Fill {
        path: b.build(),
        color: *color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xc_core::element::ElementType;

    fn shape(kind: ElementType, seed: i64) -> Element {
        Element {
            kind,
            id: format!("{kind:?}"),
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 60.0,
            seed,
            backgroundColor: "#a5d8ff".into(),
            ..Default::default()
        }
    }

    fn linear(kind: ElementType, seed: i64) -> Element {
        Element {
            kind,
            id: "lin".into(),
            x: 0.0,
            y: 0.0,
            points: Some(vec![[0.0, 0.0], [80.0, 40.0]]),
            seed,
            endArrowhead: Some("triangle".into()),
            ..Default::default()
        }
    }

    #[test]
    fn shapes_render_nonempty_and_deterministic() {
        for kind in [ElementType::Rectangle, ElementType::Ellipse, ElementType::Diamond] {
            let a = render_element(&shape(kind, 12345)).unwrap();
            let b = render_element(&shape(kind, 12345)).unwrap();
            assert!(!a.is_empty(), "{kind:?} produced no ops");
            assert_eq!(format!("{a:?}"), format!("{b:?}"), "{kind:?} not deterministic");
        }
    }

    #[test]
    fn seed_changes_geometry() {
        let a = render_element(&shape(ElementType::Rectangle, 1)).unwrap();
        let b = render_element(&shape(ElementType::Rectangle, 2)).unwrap();
        assert_ne!(format!("{a:?}"), format!("{b:?}"), "seeds must change geometry");
    }

    #[test]
    fn roughness_zero_is_still_stroked() {
        let mut el = shape(ElementType::Rectangle, 7);
        el.roughness = 0;
        let ops = render_element(&el).unwrap();
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Stroke { .. })));
    }

    #[test]
    fn solid_fill_produces_fill_op() {
        let mut el = shape(ElementType::Ellipse, 42);
        el.fillStyle = xc_core::element::FillStyle::Solid;
        let ops = render_element(&el).unwrap();
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Fill { .. })));
    }

    #[test]
    fn hachure_fill_produces_stroke_sets_in_fill_color() {
        let el = shape(ElementType::Rectangle, 42); // default fillStyle = hachure
        let ops = render_element(&el).unwrap();
        let fill_color = parse_color("#a5d8ff");
        let fill_strokes = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Stroke { color, .. } if *color == fill_color))
            .count();
        assert!(fill_strokes > 0, "hachure must emit fill-colored strokes");
    }

    #[test]
    fn arrows_emit_head_fill() {
        let ops = render_element(&linear(ElementType::Arrow, 99)).unwrap();
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Fill { .. })), "arrowhead");
        assert!(ops.iter().any(|op| matches!(op, DrawOp::Stroke { .. })), "shaft");
    }

    #[test]
    fn freedraw_is_not_roughened() {
        let mut el = linear(ElementType::Freedraw, 5);
        el.endArrowhead = None;
        let a = render_element(&el).unwrap();
        let b = render_element(&linear(ElementType::Freedraw, 999_999)).unwrap();
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "freedraw must ignore seed");
        match &a[0] {
            DrawOp::Stroke { path, .. } => {
                let approx_len = path.iter().count();
                assert!(approx_len >= 2, "polyline segments missing");
            }
            _ => panic!("expected stroke"),
        }
    }

    #[test]
    fn dashed_style_flags_dash() {
        let mut el = shape(ElementType::Rectangle, 3);
        el.strokeStyle = StrokeStyle::Dashed;
        let ops = render_element(&el).unwrap();
        assert!(ops
            .iter()
            .any(|op| matches!(op, DrawOp::Stroke { dash: Some(_), .. })));
    }

    #[test]
    fn rounded_rectangle_uses_path_generator() {
        let mut el = shape(ElementType::Rectangle, 11);
        el.roundness = Some(Roundness { kind: 3, value: Some(12.0) });
        let ops = render_rounded_rect(&el).unwrap();
        assert!(!ops.is_empty());
    }

    #[test]
    fn deleted_elements_render_nothing() {
        let mut el = shape(ElementType::Rectangle, 1);
        el.isDeleted = true;
        assert!(render_element(&el).unwrap().is_empty());
    }

    #[test]
    fn colors_parse() {
        let c = parse_color("#ff0000");
        assert!((c.red - 1.0).abs() < 1e-6);
        let c = parse_color("#00000000");
        assert_eq!(c.alpha, 0.0);
        assert_eq!(parse_color("transparent").alpha, 0.0);
    }
}
