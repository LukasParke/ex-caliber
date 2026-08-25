//! Scene → SVG rendering (precise style, no rough sketching yet — M3 brings rough
//! parity to the canvas and this module unifies with it), plus SVG → PNG via resvg.
//!
//! This powers `screenshot`/`export` for the MCP surface: agents need faithful
//! structure and styling, which precise vector output provides.

use xc_core::element::{Element, ElementType, StrokeStyle};
use xc_core::scene::Scene;

/// Render the scene's live elements to a standalone SVG document.
///
/// `padding` is world-space margin added around the content bounds.
pub fn scene_to_svg(scene: &Scene, padding: f64) -> String {
    let elements = scene.ordered();
    let bounds = content_bounds(&elements, padding);
    let mut out = String::with_capacity(16 * 1024);
    out.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" \
              width=\"{:.0}\" height=\"{:.0}\" \
              viewBox=\"{:.0} {:.0} {:.0} {:.0}\">\n",
        bounds[2], bounds[3], bounds[0], bounds[1], bounds[2], bounds[3]
    ));
    out.push_str(&format!(
        "<rect x=\"{:.0}\" y=\"{:.0}\" width=\"{:.0}\" height=\"{:.0}\" fill=\"#ffffff\"/>\n",
        bounds[0], bounds[1], bounds[2], bounds[3]
    ));

    // Frame backgrounds first, so framed content paints above them.
    for el in &elements {
        if el.kind == ElementType::Frame {
            out.push_str(&frame_svg(el));
        }
    }
    for el in &elements {
        if el.kind != ElementType::Frame {
            out.push_str(&element_svg(el, &scene.files));
        }
    }
    out.push_str("</svg>\n");
    out
}

/// `[min_x, min_y, width, height]` covering all live elements.
pub fn content_bounds(elements: &[&Element], padding: f64) -> [f64; 4] {
    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    for el in elements {
        let (w, h) = el.effective_size();
        min_x = min_x.min(el.x);
        min_y = min_y.min(el.y);
        max_x = max_x.max(el.x + w);
        max_y = max_y.max(el.y + h);
    }
    if elements.is_empty() {
        return [0.0, 0.0, 100.0, 100.0];
    }
    [
        min_x - padding,
        min_y - padding,
        (max_x - min_x) + 2.0 * padding,
        (max_y - min_y) + 2.0 * padding,
    ]
}

fn stroke_attrs(el: &Element) -> String {
    let dash = match el.strokeStyle {
        StrokeStyle::Solid => String::new(),
        StrokeStyle::Dashed => " stroke-dasharray=\"8 6\"".to_string(),
        StrokeStyle::Dotted => " stroke-dasharray=\"2 4\"".to_string(),
    };
    format!(
        " stroke=\"{}\" stroke-width=\"{}\"{} opacity=\"{}\"",
        el.strokeColor,
        el.strokeWidth,
        dash,
        el.opacity as f64 / 100.0
    )
}

fn fill_attr(el: &Element) -> String {
    if el.backgroundColor == "transparent" {
        " fill=\"none\"".to_string()
    } else {
        format!(" fill=\"{}\"", el.backgroundColor)
    }
}

fn element_svg(el: &Element, files: &serde_json::Value) -> String {
    let (w, h) = el.effective_size();
    match el.kind {
        ElementType::Rectangle => format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"{fill}{stroke}/>\n",
            x = fmt(el.x),
            y = fmt(el.y),
            w = fmt(w),
            h = fmt(h),
            fill = fill_attr(el),
            stroke = stroke_attrs(el),
        ),
        ElementType::Diamond => format!(
            "<polygon points=\"{pts}\"{fill}{stroke}/>\n",
            pts = diamond_points(el.x, el.y, w, h),
            fill = fill_attr(el),
            stroke = stroke_attrs(el),
        ),
        ElementType::Ellipse => format!(
            "<ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\"{fill}{stroke}/>\n",
            cx = fmt(el.x + w / 2.0),
            cy = fmt(el.y + h / 2.0),
            rx = fmt(w / 2.0),
            ry = fmt(h / 2.0),
            fill = fill_attr(el),
            stroke = stroke_attrs(el),
        ),
        ElementType::Frame => String::new(), // painted separately
        ElementType::Text => text_svg(el),
        ElementType::Line | ElementType::Arrow | ElementType::Freedraw => linear_svg(el),
        ElementType::Image => image_svg(el, files),
        _ => format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\"{fill}{stroke}/>\n",
            x = fmt(el.x),
            y = fmt(el.y),
            w = fmt(w),
            h = fmt(h),
            fill = fill_attr(el),
            stroke = stroke_attrs(el),
        ),
    }
}

fn image_svg(el: &Element, files: &serde_json::Value) -> String {
    let (w, h) = el.effective_size();
    let data_url = el
        .fileId
        .as_deref()
        .and_then(|fid| files.get(fid))
        .and_then(|f| f.get("dataURL"))
        .and_then(|u| u.as_str())
        .map(str::to_string);
    let Some(data_url) = data_url else {
        return format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"#f1f3f5\" \
             stroke=\"#adb5bd\" stroke-dasharray=\"4 4\"/>\n\
             <text x=\"{tx}\" y=\"{ty}\" font-family=\"sans-serif\" font-size=\"12\" \
             fill=\"#495057\">image:{id}</text>\n",
            x = fmt(el.x),
            y = fmt(el.y),
            w = fmt(w),
            h = fmt(h),
            tx = fmt(el.x + 4.0),
            ty = fmt(el.y + h / 2.0),
            id = el.fileId.as_deref().unwrap_or("?"),
        );
    };
    // Nested <svg> viewport: crop rect (natural coords) becomes the viewBox, the
    // full image fills natural dimensions — the excalidraw crop model exactly.
    let (view_box, nat) = match el.crop.as_ref() {
        Some(crop) => {
            let g = |k: &str| crop.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
            (
                format!(
                    "{} {} {} {}",
                    fmt(g("x")),
                    fmt(g("y")),
                    fmt(g("width")),
                    fmt(g("height"))
                ),
                (g("naturalWidth"), g("naturalHeight")),
            )
        }
        None => (format!("0 0 {} {}", fmt(w), fmt(h)), (w, h)),
    };
    format!(
        "<svg x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" viewBox=\"{vb}\" \
         preserveAspectRatio=\"none\" xmlns=\"http://www.w3.org/2000/svg\">\n\
         <image href=\"{href}\" x=\"0\" y=\"0\" width=\"{nw}\" height=\"{nh}\" \
         preserveAspectRatio=\"none\"/>\n</svg>\n",
        x = fmt(el.x),
        y = fmt(el.y),
        w = fmt(w),
        h = fmt(h),
        vb = view_box,
        href = xml_escape(&data_url),
        nw = fmt(nat.0),
        nh = fmt(nat.1),
    )
}

fn frame_svg(el: &Element) -> String {
    format!(
        "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"#f8f9fa\" \
         stroke=\"#ced4da\" stroke-dasharray=\"6 6\"/>\n\
         <text x=\"{tx}\" y=\"{ty}\" font-family=\"sans-serif\" font-size=\"14\" \
         fill=\"#868e96\">{name}</text>\n",
        x = fmt(el.x),
        y = fmt(el.y),
        w = fmt(el.width),
        h = fmt(el.height),
        tx = fmt(el.x + 4.0),
        ty = fmt(el.y - 6.0),
        name = xml_escape(el.name.as_deref().unwrap_or("")),
    )
}

fn text_svg(el: &Element) -> String {
    let font_size = el.fontSize.unwrap_or(20.0);
    let anchor = match el.textAlign {
        Some(xc_core::element::TextAlign::Center) => "middle",
        Some(xc_core::element::TextAlign::Right) => "end",
        _ => "start",
    };
    let (w, _) = el.effective_size();
    let tx = match anchor {
        "middle" => el.x + w / 2.0,
        "end" => el.x + w,
        _ => el.x,
    };
    let mut out = String::new();
    let line_height = el.lineHeight.unwrap_or(1.25) * font_size;
    for (i, line) in el.text.as_deref().unwrap_or("").split('\n').enumerate() {
        out.push_str(&format!(
            "<text x=\"{tx}\" y=\"{ty}\" font-family=\"sans-serif\" font-size=\"{fs}\" \
             fill=\"{color}\" text-anchor=\"{anchor}\">{escaped}</text>\n",
            tx = fmt(tx),
            ty = fmt(el.y + line_height * (i as f64 + 0.8)),
            fs = fmt(font_size),
            color = el.strokeColor,
            anchor = anchor,
            escaped = xml_escape(line),
        ));
    }
    out
}

fn linear_svg(el: &Element) -> String {
    let Some(points) = &el.points else {
        return String::new();
    };
    if points.len() < 2 {
        return String::new();
    }
    let path = points
        .iter()
        .enumerate()
        .map(|(i, [px, py])| {
            format!(
                "{}{:.1} {:.1}",
                if i == 0 { "M" } else { "L" },
                el.x + px,
                el.y + py
            )
        })
        .collect::<Vec<_>>()
        .join(" ");

    let mut out = format!(
        "<path d=\"{path}\"{fill}{stroke} stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n",
        fill = if el.kind == ElementType::Freedraw && el.backgroundColor != "transparent" {
            fill_attr(el)
        } else {
            " fill=\"none\"".to_string()
        },
        stroke = stroke_attrs(el),
    );
    // Arrowheads at bound ends.
    if el.kind == ElementType::Arrow {
        if el.endArrowhead.is_some() {
            out.push_str(&arrowhead_svg(el, points, false));
        }
        if el.startArrowhead.is_some() {
            out.push_str(&arrowhead_svg(el, points, true));
        }
    }
    out
}

fn arrowhead_svg(el: &Element, points: &[[f64; 2]], at_start: bool) -> String {
    let (tip, prev) = if at_start {
        (points[0], points[1])
    } else {
        let n = points.len();
        (points[n - 1], points[n - 2])
    };
    let angle = (tip[1] - prev[1]).atan2(tip[0] - prev[0]);
    let len = 12.0;
    let spread = 0.42; // radians, half-angle
    let p1 = [
        tip[0] - len * (angle - spread).cos(),
        tip[1] - len * (angle - spread).sin(),
    ];
    let p2 = [
        tip[0] - len * (angle + spread).cos(),
        tip[1] - len * (angle + spread).sin(),
    ];
    format!(
        "<polygon points=\"{tx:.1},{ty:.1} {x1:.1},{y1:.1} {x2:.1},{y2:.1}\" fill=\"{color}\"/>\n",
        tx = el.x + tip[0],
        ty = el.y + tip[1],
        x1 = el.x + p1[0],
        y1 = el.y + p1[1],
        x2 = el.x + p2[0],
        y2 = el.y + p2[1],
        color = el.strokeColor,
    )
}

fn diamond_points(x: f64, y: f64, w: f64, h: f64) -> String {
    format!(
        "{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}",
        x + w / 2.0,
        y,
        x + w,
        y + h / 2.0,
        x + w / 2.0,
        y + h,
        x,
        y + h / 2.0,
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// SVG → PNG at `scale` device ratio. resvg ships an empty font database by
/// default — without loading system fonts, every `<text>` silently vanishes.
pub fn svg_to_png(svg: &str, scale: f64) -> Result<Vec<u8>, String> {
    let mut fontdb = resvg::usvg::fontdb::Database::new();
    fontdb.load_system_fonts();
    fontdb.set_sans_serif_family("DejaVu Sans");
    let opts = resvg::usvg::Options {
        fontdb: std::sync::Arc::new(fontdb),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &opts).map_err(|e| e.to_string())?;
    let size = tree.size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(
        (size.width() * scale as f32).round() as u32,
        (size.height() * scale as f32).round() as u32,
    )
    .ok_or("pixmap alloc failed")?;
    let transform = resvg::tiny_skia::Transform::from_scale(scale as f32, scale as f32);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

/// Trim float noise: integers print without decimals.
fn fmt(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Crop decoded image bytes to `crop` (natural-coordinate rect: x, y, width,
/// height — the excalidraw image-crop model). Returns PNG bytes.
pub fn crop_image(data: &[u8], crop: &serde_json::Value) -> Result<Vec<u8>, String> {
    let get = |k: &str| -> Result<f64, String> {
        crop.get(k)
            .and_then(|v| v.as_f64())
            .ok_or_else(|| format!("crop missing {k}"))
    };
    let (cx, cy) = (get("x")?, get("y")?);
    let (cw, ch) = (get("width")?, get("height")?);
    if cw <= 0.0 || ch <= 0.0 {
        return Err("crop rect is empty".into());
    }
    let img = ::image::load_from_memory(data).map_err(|e| format!("decode: {e}"))?;
    let (nw, nh) = (img.width() as f64, img.height() as f64);
    let x = (cx.max(0.0) * img.width() as f64 / nw).round() as u32;
    let y = (cy.max(0.0) * img.height() as f64 / nh).round() as u32;
    let w =
        ((cw * img.width() as f64 / nw).round() as u32).min(img.width().saturating_sub(x).max(1));
    let h =
        ((ch * img.height() as f64 / nh).round() as u32).min(img.height().saturating_sub(y).max(1));
    let cropped = img.crop_imm(x, y, w, h);
    let mut out = std::io::Cursor::new(Vec::new());
    cropped
        .write_to(&mut out, ::image::ImageFormat::Png)
        .map_err(|e| format!("encode: {e}"))?;
    Ok(out.into_inner())
}

/// Decode an embedded file (`files[fileId].dataURL`) into (mime, bytes).
pub fn decode_file_data(files: &serde_json::Value, file_id: &str) -> Option<(String, Vec<u8>)> {
    let url = files.get(file_id)?.get("dataURL")?.as_str()?;
    let rest = url.strip_prefix("data:")?;
    let (mime, b64) = rest.split_once(";base64,")?;
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    Some((mime.to_string(), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use xc_core::element::Element;

    #[test]
    fn svg_contains_all_element_kinds() {
        let mut scene = Scene::new();
        for (kind, id) in [
            (ElementType::Rectangle, "r"),
            (ElementType::Ellipse, "e"),
            (ElementType::Diamond, "d"),
            (ElementType::Arrow, "a"),
            (ElementType::Text, "t"),
        ] {
            let mut el = Element {
                kind,
                id: id.to_string(),
                ..Default::default()
            };
            el.x = 10.0;
            el.y = 10.0;
            el.width = 50.0;
            el.height = 50.0;
            if el.is_linear() {
                el.points = Some(vec![[0.0, 0.0], [50.0, 50.0]]);
                el.endArrowhead = Some("triangle".into());
            }
            if el.kind == ElementType::Text {
                el.text = Some("hi".into());
            }
            scene.add(el).unwrap();
        }
        let svg = scene_to_svg(&scene, 10.0);
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<ellipse"));
        assert!(svg.contains("<polygon"));
        assert!(svg.contains("<path"));
        assert!(svg.contains("<text"));
        assert!(svg.contains("viewBox"));
    }

    #[test]
    fn decodes_embedded_file_data() {
        let files = serde_json::json!({
            "f1": {"dataURL": "data:image/png;base64,aGVsbG8="}
        });
        let (mime, bytes) = decode_file_data(&files, "f1").unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"hello");
        assert!(decode_file_data(&files, "missing").is_none());
    }

    #[test]
    fn png_export_produces_bytes() {
        let mut scene = Scene::new();
        scene
            .add(Element {
                kind: ElementType::Rectangle,
                id: "r".into(),
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
                backgroundColor: "#a5d8ff".into(),
                ..Default::default()
            })
            .unwrap();
        let svg = scene_to_svg(&scene, 5.0);
        let png = svg_to_png(&svg, 2.0).unwrap();
        assert!(png.len() > 100);
        assert_eq!(&png[1..4], b"PNG");
    }
}

#[cfg(test)]
mod crop_tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        // 4x4 red PNG via the image crate itself (round-trip source).
        let img = ::image::RgbaImage::from_pixel(4, 4, ::image::Rgba([255, 0, 0, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, ::image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn crops_to_requested_rect() {
        let crop = serde_json::json!({"x": 0.0, "y": 0.0, "width": 2.0, "height": 4.0});
        let cropped = crop_image(&tiny_png(), &crop).unwrap();
        let img = ::image::load_from_memory(&cropped).unwrap();
        assert_eq!((img.width(), img.height()), (2, 4));
    }

    #[test]
    fn empty_crop_is_an_error() {
        let crop = serde_json::json!({"x": 0.0, "y": 0.0, "width": 0.0, "height": 0.0});
        assert!(crop_image(&tiny_png(), &crop).is_err());
    }
}

#[cfg(test)]
mod svg_image_tests {
    use super::*;
    use xc_core::element::Element;

    #[test]
    fn svg_export_embeds_images_with_crop() {
        let mut scene = Scene::new();
        scene.files = serde_json::json!({
            "img1": {"dataURL": "data:image/png;base64,AAAA"}
        });
        scene
            .add(Element {
                kind: ElementType::Image,
                id: "img".into(),
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 50.0,
                fileId: Some("img1".into()),
                crop: Some(serde_json::json!({
                    "x": 5.0, "y": 5.0, "width": 50.0, "height": 25.0,
                    "naturalWidth": 100.0, "naturalHeight": 100.0
                })),
                ..Default::default()
            })
            .unwrap();
        let svg = scene_to_svg(&scene, 0.0);
        assert!(
            svg.contains("<image href=\"data:image/png;base64,AAAA\""),
            "embeds dataURL"
        );
        assert!(
            svg.contains("viewBox=\"5 5 50 25\""),
            "crop rect becomes viewBox: {}",
            svg
        );
        assert!(
            svg.contains("width=\"100\" height=\"100\""),
            "natural dims for inner image"
        );
    }

    #[test]
    fn missing_file_renders_placeholder() {
        let mut scene = Scene::new();
        scene
            .add(Element {
                kind: ElementType::Image,
                id: "img".into(),
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
                fileId: Some("gone".into()),
                ..Default::default()
            })
            .unwrap();
        let svg = scene_to_svg(&scene, 0.0);
        assert!(
            svg.contains("image:gone"),
            "placeholder names the missing file"
        );
    }
}
