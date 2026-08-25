//! The real canvas: renders a `Scene` with rough styling, infinite pan/zoom,
//! viewport culling, and text/image overlays.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::tools::{Tool, ToolState};
use gpui::{
    App, Application, Bounds, Context, ImageSource, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Render, RenderImage, ScrollDelta, ScrollWheelEvent, SharedString, Window,
    WindowBounds, WindowOptions, canvas, div, img, prelude::*, px, size,
};
use lyon::math::Transform;
use roughr::Srgba;
use xc_core::element::{Element, ElementType};
use xc_core::scene::Scene;
use xc_render::DrawOp;

/// Scene shared between the canvas and (later) the in-process MCP bridge.
pub type SharedScene = Arc<Mutex<Scene>>;

pub struct SceneCanvas {
    pub scene: SharedScene,
    pub file: Option<PathBuf>,
    pan: Point<Pixels>,
    zoom: f64,
    dragging: bool,
    /// Decoded images by fileId, filled lazily during render.
    images: HashMap<String, Option<Arc<RenderImage>>>,
    /// Interaction state machine (selection, creation, drags).
    tools: crate::tools::ToolState,
    /// Active inline text-editing session, if any.
    text_edit: Option<crate::text_edit::TextEditState>,
    focus: gpui::FocusHandle,
}

impl SceneCanvas {
    /// Load from disk (missing file → empty scene).
    pub fn load(file: Option<PathBuf>, cx: &mut App) -> Self {
        let scene = file
            .as_deref()
            .filter(|p| p.exists())
            .map(|p| {
                xc_core::file::load_scene(p).unwrap_or_else(|e| {
                    eprintln!("xc: failed to load {}: {e}", p.display());
                    Scene::new()
                })
            })
            .unwrap_or_default();
        Self {
            scene: Arc::new(Mutex::new(scene)),
            file,
            pan: Point {
                x: px(60.0),
                y: px(60.0),
            },
            zoom: 1.0,
            dragging: false,
            images: HashMap::new(),
            tools: ToolState::default(),
            text_edit: None,
            focus: cx.focus_handle().clone(),
        }
    }

    /// Atomic save to the backing file (tmp + rename): every committed gesture
    /// lands on disk, so a crash loses at most the in-flight gesture.
    fn persist(&self) {
        let Some(path) = &self.file else { return };
        let scene = match self.scene.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let body = xc_core::file::save_scene_to_string(&scene);
        let tmp = path.with_extension("excalidraw.tmp");
        if let Err(e) = std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, path)) {
            eprintln!("xc: autosave failed: {e}");
        }
    }

    /// Enter inline edit mode for a text element.
    fn begin_text_edit(&mut self, id: &str) {
        if let Some(el) = self.scene.lock().unwrap().get(id).cloned() {
            self.text_edit = Some(crate::text_edit::TextEditState::new(
                id,
                el.text.as_deref().unwrap_or(""),
            ));
            self.tools.selection.clear();
            self.tools.selection.insert(id.to_string());
        }
    }

    /// Commit the editing session: write text back, re-measure, persist.
    fn commit_text_edit(&mut self) {
        let Some(ed) = self.text_edit.take() else {
            return;
        };
        let mut scene = self.scene.lock().unwrap();
        if let Some(el) = scene.get(&ed.element_id).cloned() {
            let mut next = el.clone();
            next.text = Some(ed.text().to_string());
            next.originalText = Some(ed.text().to_string());
            xc_core::text::global_engine().reflow(&mut next);
            next.version = el.version + 1;
            next.updated = xc_core::time::now_ms();
            scene.replace_silent(next);
            drop(scene);
            self.persist();
        }
    }

    fn to_world(&self, screen: Point<Pixels>) -> (f64, f64) {
        (
            (f64::from(screen.x) - f64::from(self.pan.x)) / self.zoom,
            (f64::from(screen.y) - f64::from(self.pan.y)) / self.zoom,
        )
    }

    fn status_text(&self, visible: usize, total: usize) -> String {
        let name = self
            .file
            .as_deref()
            .map(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
            .unwrap_or_else(|| "untitled".into());
        format!(
            "{name}   zoom {:.2}\u{00d7}   elements {visible}/{total}   drag=pan  wheel=zoom",
            self.zoom
        )
    }

    fn image_for(
        &mut self,
        el: &Element,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        let file_id = el.fileId.clone()?;
        if let Some(cached) = self.images.get(&file_id) {
            return cached.clone();
        }
        let crop = el.crop.clone();
        let decoded = self
            .scene
            .lock()
            .ok()
            .and_then(|scene| xc_io::decode_file_data(&scene.files, &file_id))
            .and_then(|(mime, mut bytes)| {
                // Apply the excalidraw image crop by pre-cropping the bitmap.
                if let Some(crop) = &crop {
                    bytes = xc_io::crop_image(&bytes, crop).unwrap_or(bytes);
                }
                let format = gpui::ImageFormat::from_mime_type(&mime)?;
                Some(gpui::Image::from_bytes(format, bytes))
            })
            .and_then(|image| Arc::new(image).use_render_image(window, cx));
        self.images.insert(file_id, decoded.clone());
        decoded
    }
}

fn srgba_to_hsla(c: Srgba) -> gpui::Hsla {
    // Our parser feeds sRGB bytes into palette's component fields; round-trip
    // through the u32 hex path so displayed colors match the file exactly.
    let hex = ((c.red * 255.0).round() as u32) << 24
        | ((c.green * 255.0).round() as u32) << 16
        | ((c.blue * 255.0).round() as u32) << 8
        | ((c.alpha * 255.0).round() as u32);
    gpui::rgba(hex).into()
}

#[derive(Clone, Copy)]
struct Viewport {
    pan: Point<Pixels>,
    zoom: f64,
    width: f64,
    height: f64,
}

impl Viewport {
    fn to_screen(self, x: f64, y: f64) -> (f32, f32) {
        (
            (x * self.zoom) as f32 + f32::from(self.pan.x),
            (y * self.zoom) as f32 + f32::from(self.pan.y),
        )
    }

    /// Element bounding box (with stroke slop) intersect test in world space.
    fn sees(&self, el: &Element) -> bool {
        let (w, h) = el.effective_size();
        let slop = 24.0;
        let (min_x, min_y) = (el.x - slop, el.y - slop);
        let (max_x, max_y) = (el.x + w + slop, el.y + h + slop);
        let view_left = -f64::from(self.pan.x) / self.zoom;
        let view_top = -f64::from(self.pan.y) / self.zoom;
        let view_right = view_left + self.width / self.zoom;
        let view_bottom = view_top + self.height / self.zoom;
        min_x < view_right && max_x > view_left && min_y < view_bottom && max_y > view_top
    }
}

/// local → screen transform: pan ∘ zoom ∘ rotate-about-center.
fn element_transform(el: &Element, vp: &Viewport) -> Transform {
    let (w, h) = el.effective_size();
    let (cx, cy) = ((w / 2.0) as f32, (h / 2.0) as f32);
    let (ox, oy) = vp.to_screen(el.x, el.y);
    Transform::translation(ox, oy)
        .then_scale(vp.zoom as f32, vp.zoom as f32)
        .then_translate(lyon::math::vector(cx, cy))
        .then_rotate(lyon::geom::Angle::radians(el.angle as f32))
        .then_translate(lyon::math::vector(-cx, -cy))
}

fn paint_element(el: &Element, vp: &Viewport, window: &mut Window) {
    let ops = match xc_render::geometry::render_element(el) {
        Ok(ops) => ops,
        Err(e) => {
            eprintln!("xc: render {}: {e}", el.id);
            return;
        }
    };
    let transform = element_transform(el, vp);
    for op in ops {
        let mut builder = match &op {
            DrawOp::Fill { .. } => gpui::PathBuilder::fill(),
            DrawOp::Stroke { width, dash, .. } => {
                let mut b = gpui::PathBuilder::stroke(px(*width * vp.zoom as f32));
                if let Some(dash) = dash {
                    let dashes: Vec<Pixels> =
                        dash.iter().map(|d| px(*d * vp.zoom as f32)).collect();
                    b = b.dash_array(&dashes);
                }
                b
            }
        };
        builder.transform(transform);
        trace_path(&mut builder, op_path(&op));
        if let Ok(path) = builder.build() {
            let color = match &op {
                DrawOp::Fill { color, .. } | DrawOp::Stroke { color, .. } => *color,
            };
            window.paint_path(path, srgba_to_hsla(color));
        }
    }
}

impl SceneCanvas {
    /// Editing overlay: working text, selection box, and a caret bar at the
    /// caret position (prefix width measured by the text engine).
    fn render_text_editor(&self, el: &Element, vp: Viewport) -> gpui::AnyElement {
        let ed = self.text_edit.as_ref().unwrap();
        let engine = xc_core::text::global_engine();
        let family = xc_core::text::family_for(el.fontFamily.unwrap_or(1));
        let font_size = (el.fontSize.unwrap_or(20.0) * vp.zoom).max(4.0);
        let line_px = font_size * el.lineHeight.unwrap_or(1.25);
        let text = ed.text();

        let (sx, sy) = vp.to_screen(el.x, el.y);
        let (caret_x, caret_line) = {
            let prefix = &text[..ed.caret.min(text.len())];
            let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let col = &prefix[line_start..];
            let (w, _) = engine.measure(col, family, el.fontSize.unwrap_or(20.0), 1.0);
            let line_i = text[..ed.caret.min(text.len())].matches('\n').count();
            (
                sx + (w * vp.zoom) as f32,
                sy + (line_i as f64 * line_px * vp.zoom) as f32,
            )
        };

        let mut lines: Vec<gpui::AnyElement> = Vec::new();
        for line in text.split('\n') {
            lines.push(
                div()
                    .font_family(family)
                    .text_size(px(font_size as f32))
                    .line_height(px(line_px as f32))
                    .text_color(gpui::black())
                    .child(SharedString::from(line.to_string()))
                    .into_any_element(),
            );
        }

        div()
            .absolute()
            .left(px(sx))
            .top(px(sy))
            .border_1()
            .border_color(gpui::rgb(0x2383e2))
            .children(lines)
            .child(
                div()
                    .absolute()
                    .left(px(caret_x - sx))
                    .top(px(caret_line - sy))
                    .w(px(2.))
                    .h(px(line_px as f32))
                    .bg(gpui::rgb(0x2383e2)),
            )
            .into_any_element()
    }
}

fn paint_selection_box(el: &Element, vp: &Viewport, window: &mut Window) {
    let (w, h) = el.effective_size();
    let (sx, sy) = vp.to_screen(el.x, el.y);
    let sw = (w * vp.zoom) as f32;
    let sh = (h * vp.zoom) as f32;
    let bounds = gpui::Bounds {
        origin: Point {
            x: px(sx - 2.0),
            y: px(sy - 2.0),
        },
        size: size(px(sw + 4.0), px(sh + 4.0)),
    };
    window.paint_quad(gpui::quad(
        bounds,
        px(0.),
        gpui::transparent_black(),
        px(1.),
        gpui::rgb(0x2383e2),
        Default::default(),
    ));
}

fn paint_marquee(rect: &[f64; 4], vp: &Viewport, window: &mut Window) {
    let (sx, sy) = vp.to_screen(rect[0], rect[1]);
    let sw = ((rect[2] - rect[0]) * vp.zoom) as f32;
    let sh = ((rect[3] - rect[1]) * vp.zoom) as f32;
    window.paint_quad(gpui::quad(
        gpui::Bounds {
            origin: Point {
                x: px(sx),
                y: px(sy),
            },
            size: size(px(sw), px(sh)),
        },
        px(0.),
        gpui::Background::from(gpui::hsla(0.58, 0.85, 0.5, 0.08)),
        px(1.),
        gpui::rgb(0x2383e2),
        Default::default(),
    ));
}

fn op_path(op: &DrawOp) -> &lyon::path::Path {
    match op {
        DrawOp::Fill { path, .. } | DrawOp::Stroke { path, .. } => path,
    }
}

fn trace_path(builder: &mut gpui::PathBuilder, path: &lyon::path::Path) {
    use lyon::path::PathEvent;
    for event in path.iter() {
        match event {
            PathEvent::Begin { at } => builder.move_to(gp(at)),
            PathEvent::Line { to, .. } => builder.line_to(gp(to)),
            PathEvent::Quadratic { ctrl, to, .. } => builder.curve_to(gp(to), gp(ctrl)),
            PathEvent::Cubic {
                ctrl1, ctrl2, to, ..
            } => builder.cubic_bezier_to(gp(ctrl1), gp(ctrl2), gp(to)),
            PathEvent::End { close, .. } => {
                if close {
                    builder.close()
                }
            }
        }
    }
}

fn gp(p: lyon::math::Point) -> Point<Pixels> {
    Point {
        x: px(p.x),
        y: px(p.y),
    }
}

impl Render for SceneCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot: Vec<Element> = self
            .scene
            .lock()
            .map(|s| s.ordered().into_iter().cloned().collect())
            .unwrap_or_default();
        let total = snapshot.len();

        // Overlays are positioned elements so gpui's text stack shapes them.
        let mut overlays: Vec<gpui::AnyElement> = Vec::new();
        let mut paintable: Vec<Element> = Vec::new();
        let vp_seed = Viewport {
            pan: self.pan,
            zoom: self.zoom,
            width: 0.0,
            height: 0.0,
        };

        for el in &snapshot {
            if el.kind == ElementType::Text {
                if self
                    .text_edit
                    .as_ref()
                    .map(|ed| ed.element_id == el.id)
                    .unwrap_or(false)
                {
                    overlays.push(self.render_text_editor(el, vp_seed));
                    continue;
                }
                let (w, h) = el.effective_size();
                let (sx, sy) = vp_seed.to_screen(el.x, el.y);
                let font_size = (el.fontSize.unwrap_or(20.0) * self.zoom).max(4.0);
                let family = xc_core::text::family_for(el.fontFamily.unwrap_or(1));
                let color = if el.strokeColor == "#1e1e1e" {
                    gpui::black()
                } else {
                    srgba_to_hsla(xc_render::geometry::parse_color(&el.strokeColor))
                };
                let line_px = font_size * el.lineHeight.unwrap_or(1.25);
                // autoResize=false wraps to the element width (engine-measured).
                let text_lines: Vec<String> = if el.autoResize == Some(false) {
                    xc_core::text::global_engine().wrap(
                        el.text.as_deref().unwrap_or(""),
                        family,
                        el.fontSize.unwrap_or(20.0),
                        el.lineHeight.unwrap_or(1.25),
                        w,
                    )
                } else {
                    el.text
                        .as_deref()
                        .unwrap_or("")
                        .split('\n')
                        .map(str::to_string)
                        .collect()
                };
                let mut lines: Vec<gpui::AnyElement> = Vec::new();
                for line in text_lines {
                    lines.push(
                        div()
                            .font_family(family)
                            .text_size(px(font_size as f32))
                            .line_height(px(line_px as f32))
                            .text_color(color)
                            .child(SharedString::from(line))
                            .into_any_element(),
                    );
                }
                overlays.push(
                    div()
                        .absolute()
                        .left(px(sx))
                        .top(px(sy))
                        .w(px((w * self.zoom) as f32))
                        .h(px((h * self.zoom) as f32))
                        .children(lines)
                        .into_any_element(),
                );
                continue;
            }
            if el.kind == ElementType::Image {
                if let Some(render_image) = self.image_for(el, window, cx) {
                    let (w, h) = el.effective_size();
                    let (sx, sy) = vp_seed.to_screen(el.x, el.y);
                    overlays.push(
                        div()
                            .absolute()
                            .left(px(sx))
                            .top(px(sy))
                            .w(px((w * self.zoom) as f32))
                            .h(px((h * self.zoom) as f32))
                            .child(
                                img(ImageSource::Render(render_image))
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Fill),
                            )
                            .into_any_element(),
                    );
                }
                continue;
            }
            paintable.push(el.clone());
        }

        let pan = self.pan;
        let zoom = self.zoom;
        let visible_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let visible_counter = Arc::clone(&visible_count);
        let ghost = self.tools.ghost();
        let selection: Vec<String> = self.tools.selection_vec();
        let tool = self.tools.tool;

        let status = self.status_text(0, total);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(gpui::rgb(0xf5f5f4))
            .child(
                div()
                    .flex_grow()
                    .relative()
                    .child(
                        canvas(
                            move |bounds, _, _| bounds.size,
                            move |bounds, viewport_size, window, _| {
                                let vp = Viewport {
                                    pan,
                                    zoom,
                                    width: f64::from(viewport_size.width),
                                    height: f64::from(viewport_size.height),
                                };
                                let mut visible = 0usize;
                                for el in &paintable {
                                    if !vp.sees(el) {
                                        continue;
                                    }
                                    visible += 1;
                                    paint_element(el, &vp, window);
                                }
                                // Selection outlines.
                                for id in &selection {
                                    if let Some(el) = paintable
                                        .iter()
                                        .find(|e| &e.id == id)
                                        .or_else(|| snapshot.iter().find(|e| &e.id == id))
                                    {
                                        paint_selection_box(el, &vp, window);
                                    }
                                }
                                // In-progress gesture.
                                if let Some(draft) = &ghost.element {
                                    paint_element(draft, &vp, window);
                                }
                                if let Some(rect) = &ghost.marquee {
                                    paint_marquee(rect, &vp, window);
                                }
                                visible_counter
                                    .store(visible, std::sync::atomic::Ordering::Relaxed);
                                let _ = bounds;
                                let _ = tool;
                            },
                        )
                        .size_full(),
                    )
                    .when(!overlays.is_empty(), |d| d.children(overlays))
                    .track_focus(&self.focus)
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                            let (wx, wy) = this.to_world(ev.position);
                            if this.text_edit.is_some() {
                                this.commit_text_edit();
                                cx.notify();
                                return;
                            }
                            if ev.click_count == 2 {
                                let hit = {
                                    let scene = this.scene.lock().unwrap();
                                    let ordered = scene.ordered();
                                    xc_core::hit_test::topmost(ordered.iter().copied(), wx, wy)
                                        .filter(|e| e.kind == ElementType::Text)
                                        .map(|e| e.id.clone())
                                };
                                if let Some(id) = hit {
                                    this.begin_text_edit(&id);
                                    this.focus.focus(window);
                                    cx.notify();
                                    return;
                                }
                            }
                            this.tools.pointer_down(
                                &mut this.scene.lock().unwrap(),
                                wx,
                                wy,
                                ev.modifiers.shift,
                            );
                            this.focus.focus(window);
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                        if !this.dragging && this.tools.ghost().element.is_none() {
                            // Still route moves: hover + drag continuation need them.
                        }
                        let (wx, wy) = this.to_world(ev.position);
                        this.tools.pointer_move(
                            &mut this.scene.lock().unwrap(),
                            wx,
                            wy,
                            ev.modifiers.shift,
                        );
                        cx.notify();
                    }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                            let (wx, wy) = this.to_world(ev.position);
                            this.tools
                                .pointer_up(&mut this.scene.lock().unwrap(), wx, wy);
                            this.persist();
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                        let dy = match ev.delta {
                            ScrollDelta::Pixels(d) => f64::from(d.y),
                            ScrollDelta::Lines(d) => f64::from(d.y) * 24.0,
                        };
                        let cursor = ev.position;
                        let world_x = (f64::from(cursor.x) - f64::from(this.pan.x)) / this.zoom;
                        let world_y = (f64::from(cursor.y) - f64::from(this.pan.y)) / this.zoom;
                        this.zoom = (this.zoom * (-dy * 0.0018).exp()).clamp(0.05, 20.0);
                        this.pan.x = px((f64::from(cursor.x) - world_x * this.zoom) as f32);
                        this.pan.y = px((f64::from(cursor.y) - world_y * this.zoom) as f32);
                        cx.notify();
                    }))
                    .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _, cx| {
                        let key = ev.keystroke.key.as_str();
                        let ctrl =
                            ev.keystroke.modifiers.control || ev.keystroke.modifiers.platform;

                        // Inline text editing captures everything while active.
                        if let Some(ed) = &mut this.text_edit {
                            match key {
                                "escape" => this.commit_text_edit(),
                                "backspace" => ed.backspace(),
                                "delete" => ed.delete(),
                                "left" => ed.caret_left(),
                                "right" => ed.caret_right(),
                                "home" => ed.caret_home(),
                                "end" => ed.caret_end(),
                                "enter" => ed.newline(),
                                k if !ctrl && k.len() == 1 => {
                                    let ch = if ev.keystroke.modifiers.shift {
                                        k.to_uppercase()
                                    } else {
                                        k.to_string()
                                    };
                                    ed.input(&ch);
                                }
                                "space" => ed.input(" "),
                                _ => {}
                            }
                            cx.notify();
                            return;
                        }

                        let mut handled = true;
                        {
                            let mut scene = this.scene.lock().unwrap();
                            if ctrl {
                                match key {
                                    "z" => {
                                        if ev.keystroke.modifiers.shift {
                                            scene.redo();
                                        } else {
                                            scene.undo();
                                        }
                                    }
                                    "y" => {
                                        scene.redo();
                                    }
                                    "s" => {
                                        drop(scene);
                                        this.persist();
                                    }
                                    "d" => this.tools.duplicate_selection(&mut scene),
                                    "g" => {
                                        if ev.keystroke.modifiers.shift {
                                            this.tools.ungroup_selection(&mut scene);
                                        } else {
                                            this.tools.group_selection(&mut scene);
                                        }
                                    }
                                    "a" => {
                                        let all: std::collections::HashSet<String> =
                                            scene.ordered().iter().map(|e| e.id.clone()).collect();
                                        this.tools.selection = all;
                                    }
                                    _ => handled = false,
                                }
                            } else {
                                match key {
                                    "escape" => {
                                        this.tools.set_tool(Tool::Select);
                                        this.tools.selection.clear();
                                    }
                                    "delete" | "backspace" => {
                                        this.tools.delete_selection(&mut scene)
                                    }
                                    "enter" => {}
                                    k => {
                                        if let Some(tool) = Tool::from_key(k) {
                                            this.tools.set_tool(tool);
                                        } else {
                                            handled = false;
                                        }
                                    }
                                }
                            }
                        }
                        if handled {
                            cx.notify();
                        }
                    })),
            )
            .child(
                div()
                    .h(px(26.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .bg(gpui::rgb(0x1b1e24))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .text_color(gpui::rgb(0xc9cdd4))
                    .child(SharedString::from(status)),
            )
    }
}

pub fn open_scene_window(file: Option<PathBuf>) {
    Application::new().run(|cx: &mut App| {
        // Register the bundled Excalidraw fonts so text renders with the same
        // faces the measurement engine uses.
        let fonts: Vec<std::borrow::Cow<'static, [u8]>> = xc_core::text::bundled_fonts()
            .into_iter()
            .map(|(_, bytes)| std::borrow::Cow::Borrowed(bytes))
            .collect();
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            eprintln!("xc: font registration failed: {e}");
        }
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                focus: true,
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| SceneCanvas::load(file, cx)),
        )
        .unwrap();
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
        println!("xc: window opened");
    });
}
