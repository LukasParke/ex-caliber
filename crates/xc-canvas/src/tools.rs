//! Interaction state machine: pointer/keyboard events (world coordinates) →
//! scene mutations. Toolkit-free so drag/create/select logic is unit-testable;
//! the gpui layer translates raw events and draws overlays from the state.

use std::collections::HashSet;

use xc_core::edit::{self, DragSession};
use xc_core::element::{Binding, Element, ElementType, FixedPointBinding};
use xc_core::scene::{Result, Scene, SceneError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Rectangle,
    Diamond,
    Ellipse,
    Arrow,
    Line,
    Freedraw,
    Text,
    Eraser,
}

impl Tool {
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "v" | "1" => Tool::Select,
            "r" => Tool::Rectangle,
            "d" => Tool::Diamond,
            "o" => Tool::Ellipse,
            "a" => Tool::Arrow,
            "l" | "p" => Tool::Line,
            "x" => Tool::Freedraw,
            "t" => Tool::Text,
            _ => return None,
        })
    }
}

/// What the canvas should draw for the in-progress gesture.
#[derive(Debug, Clone, Default)]
pub struct Ghost {
    /// Element under construction (not yet committed).
    pub element: Option<Element>,
    /// Marquee rectangle in world space [x0, y0, x1, y1].
    pub marquee: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Gesture {
    None,
    /// Moving the selection; cursor position tracks the latest event.
    Move { origin: [f64; 2] },
    /// Creating a shape; start point fixed.
    Create { start: [f64; 2] },
    Sketch,
    Marquee { start: [f64; 2] },
    /// Corner resize on the single selection.
    Resize { handle: Corner, orig: [f64; 4] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// One in-flight interaction. The canvas owns this; events arrive in world space.
pub struct ToolState {
    pub tool: Tool,
    pub selection: HashSet<String>,
    gesture: Gesture,
    draft: Option<Element>,
    marquee: Option<[f64; 4]>,
    resize: Option<(String, Corner, [f64; 4])>,
    drag_session: Option<DragSession>,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            selection: HashSet::new(),
            gesture: Gesture::None,
            draft: None,
            marquee: None,
            resize: None,
            drag_session: None,
        }
    }
}

impl ToolState {
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.gesture = Gesture::None;
        self.draft = None;
        self.marquee = None;
    }

    pub fn ghost(&self) -> Ghost {
        Ghost {
            element: self.draft.clone(),
            marquee: self.marquee,
        }
    }

    pub fn selection_vec(&self) -> Vec<String> {
        let mut v: Vec<String> = self.selection.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selection.contains(id)
    }

    /// Resize handle under the cursor for the single-selected element.
    pub fn handle_at(&self, scene: &Scene, x: f64, y: f64, tol: f64) -> Option<Corner> {
        if self.selection.len() != 1 {
            return None;
        }
        let el = scene.get(self.selection.iter().next()?)?;
        let (w, h) = el.effective_size();
        let corners = [
            (Corner::TopLeft, el.x, el.y),
            (Corner::TopRight, el.x + w, el.y),
            (Corner::BottomLeft, el.x, el.y + h),
            (Corner::BottomRight, el.x + w, el.y + h),
        ];
        corners
            .iter()
            .find(|(_, cx, cy)| (cx - x).abs() <= tol && (cy - y).abs() <= tol)
            .map(|(c, _, _)| *c)
    }

    pub fn pointer_down(&mut self, scene: &mut Scene, x: f64, y: f64, shift: bool) {
        match self.tool {
            Tool::Select => {
                // Resize handle first (single selection).
                let handle = self.handle_at(scene, x, y, 8.0);
                let el = handle
                    .and_then(|_| self.selection.iter().next().cloned())
                    .and_then(|id| scene.get(&id).cloned());
                if let (Some(handle), Some(el)) = (handle, el) {
                    let (w, h) = el.effective_size();
                    let orig = [el.x, el.y, el.x + w, el.y + h];
                    self.resize = Some((el.id.clone(), handle, orig));
                    self.gesture = Gesture::Resize { handle, orig };
                    return;
                }
                let ordered = scene.ordered();
                let hit = xc_core::hit_test::topmost(ordered.iter().copied(), x, y).map(|e| e.id.clone());
                match hit {
                    Some(id) => {
                        if shift {
                            if !self.selection.insert(id.clone()) {
                                self.selection.remove(&id);
                            }
                        } else if !self.selection.contains(&id) {
                            self.selection.clear();
                            self.selection.insert(id.clone());
                        }
                        self.drag_session = Some(edit::begin_drag(scene, &self.selection_vec()));
                        self.gesture = Gesture::Move { origin: [x, y] };
                    }
                    None => {
                        if !shift {
                            self.selection.clear();
                        }
                        self.marquee = Some([x, y, x, y]);
                        self.gesture = Gesture::Marquee { start: [x, y] };
                    }
                }
            }
            Tool::Text => {
                let el = Element {
                    kind: ElementType::Text,
                    text: Some("text".into()),
                    originalText: Some("text".into()),
                    x,
                    y,
                    fontSize: Some(20.0),
                    lineHeight: Some(1.25),
                    autoResize: Some(true),
                    width: 44.0,
                    height: 25.0,
                    ..Element::default()
                };
                if let Ok(id) = scene.add(el) {
                    self.selection.clear();
                    self.selection.insert(id);
                }
                self.tool = Tool::Select;
            }
            Tool::Eraser => {
                let ordered = scene.ordered();
                if let Some(hit) = xc_core::hit_test::topmost(ordered.iter().copied(), x, y).map(|e| e.id.clone())
                {
                    let _ = scene.delete(&[hit]);
                }
            }
            Tool::Freedraw => {
                self.draft = Some(Element {
                    kind: ElementType::Freedraw,
                    points: Some(vec![[x, y]]),
                    ..Element::default()
                });
                self.gesture = Gesture::Sketch;
            }
            tool => {
                let start = [x, y];
                self.draft = Some(match tool {
                    Tool::Arrow | Tool::Line => Element {
                        kind: if tool == Tool::Arrow {
                            ElementType::Arrow
                        } else {
                            ElementType::Line
                        },
                        points: Some(vec![start, start]),
                        endArrowhead: (tool == Tool::Arrow).then(|| "triangle".to_string()),
                        ..Element::default()
                    },
                    _ => Element {
                        kind: match tool {
                            Tool::Rectangle => ElementType::Rectangle,
                            Tool::Diamond => ElementType::Diamond,
                            _ => ElementType::Ellipse,
                        },
                        ..Element::default()
                    },
                });
                self.gesture = Gesture::Create { start };
            }
        }
    }

    pub fn pointer_move(&mut self, scene: &mut Scene, x: f64, y: f64, shift: bool) {
        match self.gesture {
            Gesture::None => {}
            Gesture::Move { origin } => {
                // drag_move resets to gesture-start then translates by the total
                // delta, so repeated moves compose correctly.
                let (dx, dy) = (x - origin[0], y - origin[1]);
                if dx == 0.0 && dy == 0.0 {
                    return;
                }
                if let Some(session) = &self.drag_session {
                    edit::drag_move(scene, session, dx, dy);
                }
                let _ = shift;
            }
            Gesture::Create { start } => {
                if let Some(draft) = &mut self.draft {
                    match draft.kind {
                        ElementType::Arrow | ElementType::Line => {
                            if shift {
                                // Snap to 45° increments.
                                let (dx, dy) = (x - start[0], y - start[1]);
                                let angle = dy.atan2(dx);
                                let snapped = (angle / (std::f64::consts::FRAC_PI_4)).round()
                                    * std::f64::consts::FRAC_PI_4;
                                let len = (dx * dx + dy * dy).sqrt();
                                draft.points = Some(vec![
                                    start,
                                    [start[0] + len * snapped.cos(), start[1] + len * snapped.sin()],
                                ]);
                            } else {
                                draft.points = Some(vec![start, [x, y]]);
                            }
                        }
                        _ => {
                            draft.x = start[0].min(x);
                            draft.y = start[1].min(y);
                            let mut w = (x - start[0]).abs();
                            let mut h = (y - start[1]).abs();
                            if shift {
                                let s = w.max(h);
                                w = s;
                                h = s;
                            }
                            draft.width = w;
                            draft.height = h;
                        }
                    }
                }
            }
            Gesture::Sketch => {
                if let Some(points) = self.draft.as_mut().and_then(|d| d.points.as_mut()) {
                    points.push([x, y]);
                }
            }
            Gesture::Marquee { start } => {
                self.marquee = Some([
                    start[0].min(x),
                    start[1].min(y),
                    start[0].max(x),
                    start[1].max(y),
                ]);
                let rect = self.marquee.unwrap();
                self.selection = scene
                    .ordered()
                    .iter()
                    .filter(|e| {
                        let (w, h) = e.effective_size();
                        e.x < rect[2] && e.x + w > rect[0] && e.y < rect[3] && e.y + h > rect[1]
                    })
                    .map(|e| e.id.clone())
                    .collect();
            }
            Gesture::Resize { handle, orig } => {
                let (x0, y0, x1, y1) = (orig[0], orig[1], orig[2], orig[3]);
                let (nx0, ny0, nx1, ny1) = match handle {
                    Corner::TopLeft => (x, y, x1, y1),
                    Corner::TopRight => (x0, y, x, y1),
                    Corner::BottomLeft => (x0, y, x1, y),
                    Corner::BottomRight => (x0, y0, x, y),
                };
                self.resize = Some((
                    self.selection.iter().next().cloned().unwrap_or_default(),
                    handle,
                    [nx0.min(nx1), ny0.min(ny1), (nx1 - nx0).abs().max(1.0), (ny1 - ny0).abs().max(1.0)],
                ));
            }
        }
    }

    pub fn pointer_up(&mut self, scene: &mut Scene, _x: f64, _y: f64) {
        match self.gesture {
            Gesture::Create { start } => {
                if let Some(mut draft) = self.draft.take() {
                    let tiny = match draft.kind {
                        ElementType::Arrow | ElementType::Line => {
                            draft.points.as_ref().map(|p| p[0] == p[1]).unwrap_or(true)
                        }
                        _ => draft.width < 2.0 && draft.height < 2.0,
                    };
                    if tiny {
                        // Click without drag: drop a default-sized shape.
                        let (w, h) = default_size(draft.kind);
                        if draft.is_linear() {
                            draft.points = Some(vec![[start[0], start[1]], [start[0] + w, start[1]]]);
                        } else {
                            draft.width = w;
                            draft.height = h;
                        }
                    }
                    match scene.add(draft) {
                        Ok(id) => {
                            self.tool = Tool::Select; // excalidraw returns to select
                            self.selection.clear();
                            self.selection.insert(id);
                        }
                        Err(e) => eprintln!("xc: create failed: {e}"),
                    }
                }
            }
            Gesture::Sketch => {
                let sketch = self.draft.take().filter(|d| d.points.as_ref().map(|p| p.len()).unwrap_or(0) >= 2);
                if let Some(draft) = sketch {
                    match scene.add(draft) {
                        Ok(id) => {
                            self.tool = Tool::Select;
                            self.selection.clear();
                            self.selection.insert(id);
                        }
                        Err(e) => eprintln!("xc: sketch failed: {e}"),
                    }
                }
            }
            Gesture::Move { .. } => {
                if let Some(session) = self.drag_session.take() {
                    edit::end_drag(scene, session);
                }
            }
            Gesture::Resize { .. } => {
                if let Some((id, _, [bx, by, bw, bh])) = self.resize.take() {
                    let _ = edit::resize_element(scene, &id, bx, by, bw, bh);
                }
            }
            Gesture::Marquee { .. } => self.marquee = None,
            Gesture::None => {}
        }
        self.gesture = Gesture::None;
    }

    pub fn delete_selection(&mut self, scene: &mut Scene) {
        let ids = self.selection_vec();
        if !ids.is_empty() {
            let _ = scene.delete(&ids);
            self.selection.clear();
        }
    }

    pub fn duplicate_selection(&mut self, scene: &mut Scene) {
        let ids = self.selection_vec();
        if ids.is_empty() {
            return;
        }
        if let Ok(new_ids) = edit::duplicate(scene, &ids) {
            self.selection = new_ids.into_iter().collect();
        }
    }

    pub fn group_selection(&mut self, scene: &mut Scene) {
        let ids = self.selection_vec();
        if ids.len() >= 2 {
            let _ = edit::group(scene, &ids);
        }
    }

    pub fn ungroup_selection(&mut self, scene: &mut Scene) {
        let ids = self.selection_vec();
        if !ids.is_empty() {
            let _ = edit::ungroup(scene, &ids);
        }
    }

    /// Create an arrow bound between two element ids (shared with MCP connect).
    pub fn connect_ids(&self, scene: &mut Scene, from: &str, to: &str) -> Result<String> {
        let (Some(a), Some(b)) = (scene.get(from).cloned(), scene.get(to).cloned()) else {
            return Err(SceneError::UnknownElement(from.to_string()));
        };
        let (aw, ah) = a.effective_size();
        let (_, bh) = b.effective_size();
        let arrow = Element {
            kind: ElementType::Arrow,
            points: Some(vec![
                [a.x + aw, a.y + ah / 2.0],
                [b.x, b.y + bh / 2.0],
            ]),
            x: a.x,
            y: a.y,
            width: (b.x - (a.x + aw)).abs(),
            height: (b.y + bh / 2.0 - (a.y + ah / 2.0)).abs(),
            startBinding: Some(Binding::Fixed(FixedPointBinding {
                element_id: from.to_string(),
                fixed_point: [1.0, 0.5],
                mode: None,
            })),
            endBinding: Some(Binding::Fixed(FixedPointBinding {
                element_id: to.to_string(),
                fixed_point: [0.0, 0.5],
                mode: None,
            })),
            endArrowhead: Some("triangle".into()),
            ..Element::default()
        };
        scene.add(arrow)
    }
}

#[allow(dead_code)] // retained for marquee overlay math once ghost rendering lands
fn marquee_rect(a: [f64; 2], b: [f64; 2]) -> [f64; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
}

fn default_size(kind: ElementType) -> (f64, f64) {
    match kind {
        ElementType::Diamond => (140.0, 90.0),
        ElementType::Ellipse => (120.0, 80.0),
        ElementType::Arrow | ElementType::Line => (120.0, 0.0),
        _ => (160.0, 80.0),
    }
}
