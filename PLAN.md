# ExCaliber — Implementation Plan (v1, for review)

Working title **ExCaliber** (repo: `ex-caliber`). Renameable before M0 lands crates.

A native desktop drawing/diagramming app that is **file-compatible with Excalidraw**, built in
Rust on **GPUI** (Zed's GPU-accelerated UI framework), with a **first-class MCP server** so
local agents — Claude Code, Cursor, Codex CLI, Gemini CLI — can inspect and drive the canvas.

Pitch: *an Excalidraw-shaped canvas your agent can draw on.*

---

## 1. Goals / Non-goals

**Goals**
1. Open, edit, save real `.excalidraw` / `.excalidrawlib` files with round-trip fidelity.
2. Native performance: 60 fps pan/zoom on 10k-element scenes, instant startup.
3. Hand-drawn aesthetic via deterministic rough-style geometry (same `seed` ⇒ same strokes).
4. MCP-native: the running app exposes a local MCP server; agents mutate the same scene the
   user sees, through the same undo stack.
5. Headless mode: same engine scriptable with no window (CI, cron, agent pipelines).

**Non-goals (v1)**
- Realtime multiplayer collaboration (schema keeps `version`/`versionNonce`/`index`, so it
  stays *possible* later).
- Cloud accounts/sync. Files are plain local `.excalidraw`.
- Plugin system.
- Built-in LLM assistant panel — **decided out of scope (2026-08-24)**; any MCP client is
  the assistant.
- Pixel-identical replication of Excalidraw's WebGL renderer (structural parity yes;
  identical antialiasing/rasterization no).
- iframe/embeddable *playback* — preserved losslessly, rendered as placeholder card.

---

## 2. Evidence base (verified today, not assumed)

| Claim | Source |
|---|---|
| GPUI is on crates.io: `gpui` 0.2.2 (Apache-2.0) + `gpui_platform` split (features: `wayland`, `x11`, `font-kit`; Metal on macOS, Win32/DirectWrite on Windows). Pre-1.0, breaking changes happen | crates.io API; `zed-industries/zed` `crates/gpui/README.md` |
| GPUI supports exactly what a drawing app needs: `PathBuilder` wraps **lyon** with `FillTessellator` + `StrokeTessellator`, SVG path syntax, dash arrays, transforms | `crates/gpui/src/path_builder.rs` (read directly) |
| Official MCP Rust SDK: `rmcp` 3.1.4 (Apache-2.0), implements spec `2026-07-28`, backward compatible with `2025-11-25`; proc-macro tools (`#[tool_router]`), `ContentBlock::image` for returning PNGs, stdio + Streamable-HTTP transports | `modelcontextprotocol/rust-sdk` README |
| `roughr` 0.12 (MIT) is a maintained Rust port of rough.js — used in production by Merman (headless Mermaid renderer) for its sketchy output | crates.io; Merman v0.7.0 release notes |
| Current Excalidraw element schema (read from `packages/element/src/types.ts` on master): base props (`seed`, `version`, `versionNonce`, `index` fractional string, `groupIds`, `frameId`, `boundElements`, `isDeleted`, …); types: rectangle, diamond, ellipse, text, line, arrow, freedraw, image, frame (+magicframe, iframe, embeddable). Arrow bindings are now **`fixedPoint: [fx,fy]` + `mode: inside\|orbit\|skip`** (legacy focus/gap is gone); arrows carry `elbowed` + `fixedSegments`; freedraw carries `points`, `pressures`, `simulatePressure`, `strokeOptions`; images carry `fileId`, `status`, `scale`, `crop` | excalidraw monorepo (restructured: `packages/{element,math,common,fractional-indexing,utils,…}`) |
| Claude Code registers local servers via `claude mcp add <name> -- <command…>` (stdio, default) or `--transport http <url>`; scopes: local/user/project | MCP docs + Claude Code guides |

Consequence: the whole plan sits on permissively-licensed (Apache-2.0/MIT) foundations —
no GPL contamination from Zed-the-app, since the gpui *crate* is Apache-2.0.

---

## 3. Architecture

Cargo workspace, one binary, six crates:

```text
ex-caliber/
├── Cargo.toml            # workspace
├── crates/
│   ├── xc-core/          # Scene model: element types (exact Excalidraw schema),
│   │                     # fractional indexing, geometry/hit-testing, bindings,
│   │                     # undo/redo command stack, change events. Zero GPUI deps.
│   ├── xc-io/            # .excalidraw / .excalidrawlib parse+restore(repair)+save,
│   │                     # SVG generation, PNG export (own SVG -> resvg), dataURL/image files
│   ├── xc-render/        # element -> lyon path ops (roughr styling), glyph runs,
│   │                     # tessellation caches, viewport culling. Thin over GPUI APIs.
│   ├── xc-canvas/        # GPUI canvas view: pan/zoom, tools, selection/transform handles,
│   │                     # snapping/guides, inline text editing, panels (inspector/library)
│   ├── xc-mcp/           # rmcp server: tools/resources/prompts; UDS bridge into the app;
│   │                     # headless scene host for no-GUI operation
│   └── xc/               # bin: `xc` (GUI launch, `xc mcp`, `xc headless`, `xc export`,
│                         # `xc install-claude`)
```

**Threading & concurrency**
- Scene lives in one GPUI `Entity` on the main thread; render reads it during paint.
- MCP bridge (`xc-mcp`) runs on tokio; talks to the app over a Unix domain socket in
  `$XDG_RUNTIME_DIR` (0600). Every MCP mutation is applied as a normal command ⇒ appears in
  the same undo history, triggers the same repaint/dirty pipeline. Single writer, no locks.
- No GUI running ⇒ `xc mcp` hosts the scene headlessly in-process and persists to a file.

**Determinism**: roughr consumes the element's stored `seed`; rendering is reproducible,
which also makes golden-image tests meaningful.

---

## 4. Excalidraw compatibility strategy

- **Typed mirror of the schema** above (serde; unknown fields preserved via `customData`
  plus a raw-extension map so we never drop data we don't model).
- **Restore semantics**: port Excalidraw's `restore()` behavior — coerce bad values, backfill
  missing fields, regenerate missing `index`/`seed`, drop unrecoverable elements gracefully.
  Corrupt-ish files from the wild must open.
- **Envelope**: `{ type: "excalidraw", version: 2, source, elements, appState, files }`.
  Exact `appState` allowlist + SVG/PNG scene-embedding mechanics verified in M1/M6 against
  `packages/excalidraw/src/data/*` (flagged task, not assumed).
- **Fractional indexing**: implement rocicorp `generateKeyBetween` (Apache-2.0; algorithm is
  small) — array order and `index` strings kept mutually consistent on every mutation, like
  Excalidraw's `syncMovedIndices`.
- **Bindings**: arrows bind via `fixedPoint` + `mode`; moving/resizing a bound shape
  recomputes connected arrow endpoints (orbit routing first; elbow routing in M7).
- **Golden-corpus tests**: a directory of real-world `.excalidraw` files (excalidraw repo
  fixtures + collected samples). Invariant: `load → save → load` is logically identity;
  saved output opens on excalidraw.com (manual check each milestone, scripted where possible).

**Accepted divergences (documented, not bugs)**: font metric drift on wrapped text (we
re-measure on edit; persisted boxes stay untouched), rasterizer differences, iframe
placeholders, elbow-arrow editing deferred (data preserved).

---

## 5. Rendering pipeline

Per frame: cull visible elements → for each, resolve cache (key: element render-hash ∔
zoom-bucket ∔ dpi) → produce lyon paths via `PathBuilder` → GPUI paints.

- **Shapes**: roughr emits sketch lines/fill ops per `roughness`/`seed`;
  hachure/cross-hatch/zigzag fills = generated stroke sets. `roundness` maps to arc/curve
  corners before roughening. `strokeStyle` solid/dashed/dotted → lyon dash arrays.
- **Text**: GPUI text system with bundled fonts — Excalifont (default), Nunito, Comic Shanns
  (licenses verified OFL before bundling; graceful fallback otherwise). Unitless `lineHeight`
  semantics matched. Inline editing in-canvas (basic LTR first; RTL/IME later milestone).
- **Images**: dataURL/base64 → `image` decode → GPUI image primitive; `scale` flips, `crop`
  respected when rendering (crop *editing* deferred with elbow arrows).
- **Frames**: clip children to frame bounds; frame title renders outside top-left.
- **Performance budget**: 10k mixed elements, 60 fps pan/zoom on integrated graphics;
  viewport culling + tessellation cache + LOD (drop hachure detail below ~35% zoom scale).

---

## 6. Interaction model (mirrors Excalidraw muscle memory)

- Tools + defaults keys: select `V/1`, rectangle `R`, diamond `D`, ellipse `O/E`, arrow `A`,
  line `L/P`, freedraw `X`, text `T`, image `9`, eraser `E`, frame `F`, hand `H`, laser `K`.
- Selection: click, shift-add, marquee, double-click group-entry, `Ctrl+A`.
- Transform: 8 resize handles + rotate handle; multi-select proportional resize;
  alt-drag duplicate; arrow-key nudge (+shift ×10).
- Snapping: grid toggle; center/edge alignment guides with equal-spacing hints.
- Z-order: `Ctrl+Shift+]`/`[` etc.; grouping `Ctrl+G`/`Ctrl+Shift+G`.
- Undo/redo `Ctrl+Z` / `Ctrl+Shift+Z` — shared stack, MCP mutations included.
- Clipboard: internal JSON between windows; paste of `.excalidraw` JSON text; system image
  paste creates image elements.
- Inspector panel: stroke/fill/roughness/opacity/layers/font; library panel reads
  `.excalidrawlib`.

---

## 7. MCP surface (the differentiator)

**Transports**: stdio bridge is primary —
`claude mcp add excaliber -- ~/.local/bin/xc mcp` — the bridge forwards over UDS to the
running GUI app (or hosts headless). Optional `xc mcp --http 127.0.0.1:PORT` (random token
printed once) for hosts that only speak HTTP. Never binds non-loopback.

**Agent ergonomics rules**: batch-oriented (array ops, one round trip), always return
assigned ids, spatial queries so agents don't do coordinate math blind, and a screenshot
tool so vision-capable agents can close their own feedback loop.

Draft tool set (rmcp `#[tool_router]`, schemas derived from typed params):

| Tool | Purpose |
|---|---|
| `get_scene` | summary + filtered elements (by type/id/region/text-match/frame) |
| `create_elements` | insert Excalidraw JSON fragments; returns ids (+auto `index`/`seed`) |
| `update_elements` | patch by id (deep-merge; binding-aware) |
| `delete_elements` / `duplicate_elements` | with group/binding fixups |
| `connect` | create arrow bound between two shapes (auto anchors, label option) |
| `add_text` | free or container-bound label (auto-fit) |
| `group` / `ungroup` / `reorder` / `align_distribute` | structure ops |
| `import_file` / `export_file` | `.excalidraw`/`.svg`/`.png` paths; export returns bytes too |
| `screenshot` | viewport or element bbox → PNG `ContentBlock::image` |
| `undo` / `redo` | shared history |
| `focus` | pan/zoom viewport to region (so the human sees what the agent did) |

**Resources**: `scene://current` (live JSON), `scene://summary`. **Prompts**: starter
templates ("explain-this-diagram", "erd-from-tables").

Every mutation flows through the same command stack as UI actions: undo works, autosave
fires, the human watches it happen live.

---

## 8. Milestones

Each milestone ends shippable; acceptance criteria are the contract.

- **M0 — Scaffold & GPUI spike** · workspace, cross-platform CI (fmt/clippy/test on Linux +
  macOS from the first commit; Windows compile gate), pinned gpui revision policy. Spike:
  window, infinite pan/zoom grid, 10k quads at 60 fps, input latency measured. *Exit*:
  spike meets budget on Linux; matrix green; versions locked in `Cargo.toml` with rationale.
- **M1 — Core model & file IO** · full typed schema, restore(), fractional indexing, undo
  stack, golden-corpus round-trip tests. *Exit*: every corpus file loads+saves; a saved file
  opens correctly on excalidraw.com (manual, documented).
- **M2 — Headless MCP slice** ⚡ · `xc mcp` stdio with `get/create/update/delete/connect/
  screenshot(placeholder-free SVG→PNG)/export`; Claude Code registered; end-to-end demo:
  *agent builds a 10-node architecture diagram unattended*. *Exit*: recorded session;
  mutations survive restart via file persistence. Pulled ahead of GUI deliberately — it
  de-risks the headline feature and dogfoods the model API the canvas will share.
- **M3 — Canvas rendering** · static scene render of all element types (rough styling,
  text, images, frames), pan/zoom/culling/cache. *Exit*: side-by-side visual sanity vs
  Excalidraw on corpus files; perf budget met.
- **M4 — Editing interactions** · creation tools/shortcuts, selection, move/resize/rotate,
  arrow draw + live binding, line point editing, groups, z-order, undo wiring, snapping
  basics, clipboard. *Exit*: rebuild any diagram in corpus by hand at fluent speed.
- **M5 — Text & fonts deep** · inline editing, wrapping (`autoResize:false`), container
  labels, font family/size/align controls, bundled fonts. *Exit*: text round-trips byte-clean.
- **M6 — Export, autosave, packaging** · SVG (embedded scene metadata — mechanics verified
  here), PNG via resvg, drag-drop import, crash-safe autosave journal, single-instance,
  Linux packaging (AUR PKGBUILD + AppImage; flatpak later), `install-claude` command.
  *Exit*: distro-installable artifact; autosave survives `kill -9`.
- **M7 — Advanced compatibility** · elbow-arrow editing (routing + fixed segments), image
  crop editing, embeddable placeholder cards, RTL/IME text pass.
- **M8 — Hardening** · fuzzed parsers, proptest suites, perf regression bench, macOS
  (unsigned dmg) + Windows artifacts, docs site skeleton.

Cross-cutting from M0: `cargo clippy -D warnings`, insta SVG snapshots, proptest round-trip
property, gpui `TestAppContext` interaction smokes where practical, MCP e2e script.

---

## 9. Risks & mitigations

| Risk | Mitigation |
|---|---|
| gpui pre-1.0 API churn | Pin exact rev; all gpui touch confined to `xc-render`/`xc-canvas`; track releases; vendor-patch if blocked |
| Font metric drift vs Excalidraw | Persisted geometry untouched; re-measure only on edit; divergence documented |
| Rough-parity imperfection | Seed-determinism ⇒ structural parity; pixel-parity explicitly non-goal |
| Elbow-arrow router complexity | Data preserved from day 1; editing deferred to M7 |
| Tessellation cost at scale | Cull + cache + LOD; budget enforced by bench in CI |
| rmcp/spec movement | Pin minor; SDK is spec-current (2026-07-28) and actively maintained |
| Scope creep (collab/cloud/plugins) | §1 non-goals are contractual for v1 |

---

## 10. Decisions requested from reviewer

**Resolved 2026-08-24 (reviewer):**

1. Agent scope: **MCP-only** for v1 — no built-in local-LLM panel; any MCP client is the
   assistant. (Recorded as a non-goal in §1.)
2. Platforms: **cross-platform CI gates from M0** — Linux/macOS compile+test matrix from the
   first commit; platform installers still land M6 (Linux) / M8 (macOS, Windows).
3. License: **MIT OR Apache-2.0** dual.
4. Name: **ExCaliber** confirmed — binary `xc`, crates `xc-*`.
