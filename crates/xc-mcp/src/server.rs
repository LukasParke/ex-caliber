//! The MCP tool server. One `XcMcpServer` owns a `Scene` (optionally backed by a
//! `.excalidraw` file) and exposes it over any rmcp transport — stdio for Claude
//! Code, streamable HTTP later for web-hosted agents.

use std::path::PathBuf;
use std::sync::Mutex;

use base64::Engine as _;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock};
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use xc_core::element::{Binding, Element, FixedPointBinding, LocalPoint};
use xc_core::scene::{ReorderTarget, Scene};
use xc_io as io;

#[derive(Debug, Clone)]
pub struct XcServerConfig {
    /// Backing `.excalidraw` file; loaded at start, rewritten after every mutation.
    pub file: Option<PathBuf>,
}

#[derive(Clone)]
pub struct XcMcpServer {
    scene: std::sync::Arc<Mutex<Scene>>,
    path: Option<std::sync::Arc<PathBuf>>,
}

impl XcMcpServer {
    pub fn new(config: XcServerConfig) -> Result<Self, String> {
        let scene = match &config.file {
            Some(path) if path.exists() => {
                let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
                xc_core::file::load_document(&raw).map_err(|e| e.to_string())?
            }
            _ => Scene::new(),
        };
        Ok(Self {
            scene: std::sync::Arc::new(Mutex::new(scene)),
            path: config.file.map(std::sync::Arc::new),
        })
    }

    /// Serve MCP over stdio (the Claude Code default transport).
    pub async fn serve_stdio(self) -> Result<(), String> {
        let service = self
            .serve(stdio())
            .await
            .map_err(|e| format!("mcp serve failed: {e}"))?;
        service
            .waiting()
            .await
            .map_err(|e| format!("mcp waiting failed: {e}"))?;
        Ok(())
    }

    fn persist(&self, scene: &Scene) -> Result<(), CallToolResult> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let body = xc_core::file::save_scene_to_string(scene);
        let tmp = path.with_extension("excalidraw.tmp");
        std::fs::write(&tmp, body)
            .and_then(|_| std::fs::rename(&tmp, path.as_ref()))
            .map_err(|e| tool_error(format!("persist failed: {e}")))
    }

    fn with_scene<T>(
        &self,
        f: impl FnOnce(&mut Scene) -> Result<T, String>,
    ) -> Result<T, CallToolResult> {
        let mut scene = self.scene.lock().map_err(|_| {
            tool_error("scene lock poisoned")
        })?;
        let out = f(&mut scene).map_err(tool_error)?;
        self.persist(&scene)?;
        Ok(out)
    }

    /// Run a mutation and wrap its JSON summary as a tool result; failures become
    /// tool-level errors (isError=true) so agents see the actual message.
    fn scene_tool(
        &self,
        f: impl FnOnce(&mut Scene) -> Result<Value, String>,
    ) -> Result<CallToolResult, McpError> {
        Ok(self.with_scene(f)
            .map(|v| CallToolResult::success(vec![ContentBlock::text(v.to_string())]))
            .unwrap_or_else(std::convert::identity))
    }

    fn snapshot(&self) -> Result<Scene, McpError> {
        self.scene
            .lock()
            .map(|s| s.clone())
            .map_err(|_| McpError::internal_error("scene lock poisoned", None))
    }
}

// ---- tool parameter types (schemars derives build the MCP input schemas) ----

#[derive(Deserialize, JsonSchema)]
pub struct GetSceneParams {
    /// Only return elements of this type (e.g. "rectangle", "text").
    #[serde(default)]
    pub type_filter: Option<String>,
    /// Include tombstoned elements (rarely useful).
    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateElementsParams {
    /// Excalidraw element JSON fragments. Missing bookkeeping (id/seed/index) is
    /// filled in; unknown fields are preserved. Returns assigned ids in order.
    pub elements: Vec<Value>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateElementsParams {
    /// Patches applied per id: top-level fields merge onto the existing element.
    pub updates: Vec<ElementPatch>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ElementPatch {
    pub id: String,
    /// Fields to set, e.g. {"x": 10, "backgroundColor": "#a5d8ff"}.
    pub fields: Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteElementsParams {
    /// Ids to delete. Bound arrows and container labels travel with their targets.
    pub ids: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ConnectParams {
    /// Source shape id (arrow leaves this element, bound).
    pub from_id: String,
    /// Target shape id (arrow enters this element, bound).
    pub to_id: String,
    /// Optional label rendered at the arrow midpoint.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub start_arrowhead: Option<String>,
    #[serde(default)]
    pub end_arrowhead: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct AddTextParams {
    pub text: String,
    /// World-space position (ignored when container_id is set).
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    /// Bind the label inside an existing shape (auto-centered).
    #[serde(default)]
    pub container_id: Option<String>,
    #[serde(default)]
    pub font_size: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReorderParams {
    pub id: String,
    /// front = topmost, back = bottommost, or relative to another element id.
    pub target: String,
    #[serde(default)]
    pub relative_to: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EmptyParams {}

#[derive(Deserialize, JsonSchema)]
pub struct ScreenshotParams {
    /// Restrict capture to these element ids (bbox fit); default: whole scene.
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    /// Device scale for the PNG (default 2 = retina-crisp).
    #[serde(default)]
    pub scale: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ExportFileParams {
    /// svg | png | excalidraw
    pub format: String,
    /// Write here; when omitted the bytes are returned inline instead.
    #[serde(default)]
    pub path: Option<String>,
}

#[tool_router]
impl XcMcpServer {
    #[tool(
        description = "Read the current scene: element list (id, type, geometry, style, text, bindings). Filter by type or read everything."
    )]
    fn get_scene(&self, Parameters(p): Parameters<GetSceneParams>) -> Result<CallToolResult, McpError> {
        let elements: Vec<Value> = self
            .snapshot()?
            .elements_iter()
            .into_iter()
            .filter(|e| p.include_deleted || !e.isDeleted)
            .filter(|e| p.type_filter.as_deref().is_none_or(|t| e.kind.as_str() == t))
            .map(|e| serde_json::to_value(e).expect("element serializes"))
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            serde_json::to_string_pretty(&json!({
                "count": elements.len(),
                "elements": elements,
            }))
            .expect("summary serializes"),
        )]))
    }

    #[tool(
        description = "Create one or more elements from Excalidraw JSON fragments. Accepts partial elements (x/y/width/height + type minimum); ids, seeds and z-order are assigned and returned as {index -> id}."
    )]
    fn create_elements(
        &self,
        Parameters(p): Parameters<CreateElementsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.scene_tool(|scene| {
            let mut ids = Vec::with_capacity(p.elements.len());
            for frag in &p.elements {
                if frag.get("type").and_then(Value::as_str).is_none() {
                    return Err("element fragment missing `type`".into());
                }
                let el: Element = serde_json::from_value(frag.clone())
                    .map_err(|e| format!("bad element fragment: {e}"))?;
                let id = scene.add(el).map_err(|e| e.to_string())?;
                ids.push(id);
            }
            Ok(json!({ "ids": ids }))
        })
    }

    #[tool(
        description = "Patch existing elements by id. Top-level fields merge; use for moves, resizes, style and text changes. Bumps version and updates undo history."
    )]
    fn update_elements(
        &self,
        Parameters(p): Parameters<UpdateElementsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.scene_tool(|scene| {
            let mut updated = Vec::with_capacity(p.updates.len());
            for patch in &p.updates {
                let current = scene
                    .get(&patch.id)
                    .ok_or_else(|| format!("unknown id: {}", patch.id))?;
                let mut value = serde_json::to_value(current).expect("element serializes");
                let obj = value
                    .as_object_mut()
                    .ok_or("element is not an object")?;
                let fields = patch
                    .fields
                    .as_object()
                    .ok_or("fields must be an object")?;
                for (k, v) in fields {
                    obj.insert(k.clone(), v.clone());
                }
                let next: Element = serde_json::from_value(value)
                    .map_err(|e| format!("patch produced invalid element: {e}"))?;
                scene.replace(next).map_err(|e| e.to_string())?;
                updated.push(patch.id.clone());
            }
            Ok(json!({ "updated": updated }))
        })
    }

    #[tool(
        description = "Delete elements by id. Arrows bound to deleted shapes and labels inside deleted containers are deleted too."
    )]
    fn delete_elements(
        &self,
        Parameters(p): Parameters<DeleteElementsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.scene_tool(|scene| {
            scene.delete(&p.ids).map_err(|e| e.to_string())?;
            Ok(json!({ "deleted": p.ids }))
        })
    }

    #[tool(
        description = "Create an arrow bound between two shapes: auto-anchors at the facing borders, optional label at the midpoint. Prefer this over raw create_elements for relationships."
    )]
    fn connect(
        &self,
        Parameters(p): Parameters<ConnectParams>,
    ) -> Result<CallToolResult, McpError> {
        self.scene_tool(|scene| {
            let (from, to) = (
                scene
                    .get(&p.from_id)
                    .cloned()
                    .ok_or_else(|| format!("unknown from_id: {}", p.from_id))?,
                scene
                    .get(&p.to_id)
                    .cloned()
                    .ok_or_else(|| format!("unknown to_id: {}", p.to_id))?,
            );
            let (start_fp, _) = anchor_points(&from, &to);
            let (end_fp, _) = anchor_points(&to, &from);
            let (fw, fh) = from.effective_size();
            let (tw, th) = to.effective_size();
            // Arrow runs center-to-center; endpoints trimmed to the anchor ratios.
            let a = [from.x + start_fp[0] * fw, from.y + start_fp[1] * fh];
            let b = [to.x + end_fp[0] * tw, to.y + end_fp[1] * th];
            let arrow = Element {
                kind: xc_core::element::ElementType::Arrow,
                points: Some(vec![
                    [a[0] - from.x, a[1] - from.y],
                    [b[0] - from.x, b[1] - from.y],
                ]),
                x: from.x,
                y: from.y,
                width: (b[0] - a[0]).abs(),
                height: (b[1] - a[1]).abs(),
                startBinding: Some(Binding::Fixed(FixedPointBinding {
                    element_id: p.from_id.clone(),
                    fixed_point: start_fp,
                    mode: Some(xc_core::element::BindMode::Orbit),
                })),
                endBinding: Some(Binding::Fixed(FixedPointBinding {
                    element_id: p.to_id.clone(),
                    fixed_point: end_fp,
                    mode: Some(xc_core::element::BindMode::Orbit),
                })),
                startArrowhead: p.start_arrowhead.clone(),
                endArrowhead: p.end_arrowhead.clone().or_else(|| Some("triangle".into())),
                ..Default::default()
            };
            let mut tx = scene.transaction();
            let arrow_id = tx.add(arrow).map_err(|e| e.to_string())?;
            tx.sync_binding_ref(&p.from_id, &arrow_id, true)
                .map_err(|e| e.to_string())?;
            tx.sync_binding_ref(&p.to_id, &arrow_id, true)
                .map_err(|e| e.to_string())?;

            if let Some(label) = &p.label {
                let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                let text = Element {
                    kind: xc_core::element::ElementType::Text,
                    text: Some(label.clone()),
                    originalText: Some(label.clone()),
                    // Sit above the shaft; centered-on-midpoint reads as strikethrough.
                    x: mid[0] - 20.0,
                    y: mid[1] - 38.0,
                    width: 40.0,
                    height: 24.0,
                    ..Default::default()
                };
                tx.add(text).map_err(|e| e.to_string())?;
            }
            tx.commit();
            Ok(json!({ "arrow_id": arrow_id }))
        })
    }

    #[tool(
        description = "Add a text element at a position, or bind it as a centered label inside a container shape."
    )]
    fn add_text(
        &self,
        Parameters(p): Parameters<AddTextParams>,
    ) -> Result<CallToolResult, McpError> {
        self.scene_tool(|scene| {
            let font_size = p.font_size.unwrap_or(20.0);
            let lines = p.text.split('\n').count() as f64;
            let line_height = 1.25;
            let est_width = p.text.len() as f64 * font_size * 0.55;
            let est_height = lines * font_size * line_height;
            let (x, y, container_id) = if let Some(cid) = &p.container_id {
                let c = scene
                    .get(cid)
                    .cloned()
                    .ok_or_else(|| format!("unknown container: {cid}"))?;
                let (cw, ch) = c.effective_size();
                (c.x + (cw - est_width) / 2.0, c.y + (ch - est_height) / 2.0, Some(cid.clone()))
            } else {
                (
                    p.x.ok_or("x required when container_id is absent")?,
                    p.y.ok_or("y required when container_id is absent")?,
                    None,
                )
            };
            let el = Element {
                kind: xc_core::element::ElementType::Text,
                text: Some(p.text.clone()),
                originalText: Some(p.text.clone()),
                fontSize: Some(font_size),
                lineHeight: Some(line_height),
                autoResize: Some(true),
                containerId: container_id,
                x,
                y,
                width: est_width,
                height: est_height,
                ..Default::default()
            };
            let id = scene.add(el).map_err(|e| e.to_string())?;
            Ok(json!({ "id": id }))
        })
    }

    #[tool(description = "Change paint order: target is front|back|before|after (with relative_to id).")]
    fn reorder(
        &self,
        Parameters(p): Parameters<ReorderParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = match (p.target.as_str(), p.relative_to) {
            ("front", _) => ReorderTarget::Front,
            ("back", _) => ReorderTarget::Back,
            ("before", Some(other)) => ReorderTarget::Before(other),
            ("after", Some(other)) => ReorderTarget::After(other),
            _ => return Err(McpError::invalid_params("target must be front|back|before|after with relative_to for the latter two", None)),
        };
        self.scene_tool(|scene| {
            scene.reorder(&p.id, target).map_err(|e| e.to_string())?;
            Ok(json!({ "reordered": p.id }))
        })
    }

    #[tool(description = "Undo the last change (agent and human edits share one history).")]
    fn undo(&self, Parameters(_): Parameters<EmptyParams>) -> Result<CallToolResult, McpError> {
        self.scene_tool(|scene| Ok(json!({ "undone": scene.undo() })))
    }

    #[tool(description = "Redo a previously undone change.")]
    fn redo(&self, Parameters(_): Parameters<EmptyParams>) -> Result<CallToolResult, McpError> {
        self.scene_tool(|scene| Ok(json!({ "redone": scene.redo() })))
    }


    #[tool(
        description = "Render the scene (or the bounding box of specific element ids) to PNG. Use this to visually check your work."
    )]
    fn screenshot(
        &self,
        Parameters(p): Parameters<ScreenshotParams>,
    ) -> Result<CallToolResult, McpError> {
        let scene = self.snapshot()?;
        let elements: Vec<xc_core::element::Element> = match &p.ids {
            Some(ids) => {
                let mut picked = Vec::with_capacity(ids.len());
                for id in ids {
                    picked.push(
                        scene
                            .get(id)
                            .cloned()
                            .ok_or_else(|| McpError::invalid_params(format!("unknown id: {id}"), None))?,
                    );
                }
                picked
            }
            None => scene
                .elements_iter()
                .into_iter()
                .filter(|e| !e.isDeleted)
                .cloned()
                .collect(),
        };
        if elements.is_empty() {
            return Err(McpError::invalid_params("nothing to capture", None));
        }
        let refs: Vec<&xc_core::element::Element> = elements.iter().collect();
        let svg = io::scene_to_svg(&scene_for_bounds(&refs), 12.0);
        let png = io::svg_to_png(&svg, p.scale.unwrap_or(2.0))
            .map_err(|e| McpError::internal_error(format!("render failed: {e}"), None))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
        Ok(CallToolResult::success(vec![
            ContentBlock::text(format!("captured {} elements", elements.len())),
            ContentBlock::image(b64, "image/png"),
        ]))
    }

    #[tool(
        description = "Export the scene as svg, png or excalidraw JSON — to a path, or inline when no path given."
    )]
    fn export_file(
        &self,
        Parameters(p): Parameters<ExportFileParams>,
    ) -> Result<CallToolResult, McpError> {
        let scene = self.snapshot()?;
        let inline = |mime: &str, data: Vec<u8>| {
            Ok(CallToolResult::success(vec![ContentBlock::image(
                base64::engine::general_purpose::STANDARD.encode(data),
                mime,
            )]))
        };
        match p.format.as_str() {
            "svg" => {
                let svg = io::scene_to_svg(&scene, 12.0);
                match &p.path {
                    Some(path) => {
                        std::fs::write(path, &svg)
                            .map_err(|e| McpError::internal_error(format!("write failed: {e}"), None))?;
                        Ok(CallToolResult::success(vec![ContentBlock::text(format!("wrote {path}"))]))
                    }
                    None => inline("image/svg+xml", svg.into_bytes()),
                }
            }
            "png" => {
                let svg = io::scene_to_svg(&scene, 12.0);
                let png = io::svg_to_png(&svg, 2.0)
                    .map_err(|e| McpError::internal_error(format!("render failed: {e}"), None))?;
                match &p.path {
                    Some(path) => {
                        std::fs::write(path, &png)
                            .map_err(|e| McpError::internal_error(format!("write failed: {e}"), None))?;
                        Ok(CallToolResult::success(vec![ContentBlock::text(format!("wrote {path}"))]))
                    }
                    None => inline("image/png", png),
                }
            }
            "excalidraw" => {
                let body = xc_core::file::save_scene_to_string(&scene);
                match &p.path {
                    Some(path) => {
                        std::fs::write(path, &body)
                            .map_err(|e| McpError::internal_error(format!("write failed: {e}"), None))?;
                        Ok(CallToolResult::success(vec![ContentBlock::text(format!("wrote {path}"))]))
                    }
                    None => Ok(CallToolResult::success(vec![ContentBlock::text(body)])),
                }
            }
            other => Err(McpError::invalid_params(
                format!("unsupported format: {other} (svg|png|excalidraw)"),
                None,
            )),
        }
    }
}

#[tool_handler(
    name = "excaliber",
    version = "0.1.0",
    instructions = "ExCaliber: an Excalidraw-compatible diagramming canvas. Typical loop: create_elements / connect / add_text -> screenshot to check your work -> export_file. All changes are undoable and saved to the backing .excalidraw file."
)]
impl ServerHandler for XcMcpServer {}

/// The screenshot path renders a filtered subset; build a throwaway scene view
/// carrying exactly those elements (bounds math + painter reuse).
fn scene_for_bounds(elements: &[&Element]) -> Scene {
    let mut s = Scene::new();
    for el in elements {
        let _ = s.add_silent((*el).clone());
    }
    s
}

fn tool_error(msg: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(msg.into())])
}

/// Anchor ratios (0..1 within the shape) of `from`'s border facing `to`'s center,
/// plus the absolute anchor point.
fn anchor_points(from: &Element, to: &Element) -> ([f64; 2], LocalPoint) {
    let (fw, fh) = from.effective_size();
    let (tw, th) = to.effective_size();
    let fc = [from.x + fw / 2.0, from.y + fh / 2.0];
    let tc = [to.x + tw / 2.0, to.y + th / 2.0];
    let dx = tc[0] - fc[0];
    let dy = tc[1] - fc[1];
    // Intersect the center line with the rectangle border, expressed as ratios.
    let (mut fx, mut fy) = if dx.abs() * fh >= dy.abs() * fw {
        // exits through a vertical side
        (
            0.5 + 0.5 * dx.signum(),
            if dx != 0.0 { 0.5 + 0.5 * (dy / dx) * (fw / fh) } else { 0.5 },
        )
    } else {
        (
            if dy != 0.0 { 0.5 + 0.5 * (dx / dy) * (fh / fw) } else { 0.5 },
            0.5 + 0.5 * dy.signum(),
        )
    };
    fx = fx.clamp(0.0, 1.0);
    fy = fy.clamp(0.0, 1.0);
    ([fx, fy], [fx * fw, fy * fh])
}



/// Convenience for the binary: run the stdio server to completion.
pub async fn run_stdio(config: XcServerConfig) -> Result<(), String> {
    XcMcpServer::new(config)?.serve_stdio().await
}
