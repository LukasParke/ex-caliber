//! Headless text measurement and wrapping against the bundled Excalidraw fonts.
//!
//! Determinism contract: the engine's font database contains ONLY our bundled
//! fonts — never the host system's — so measurements are identical on every
//! machine and stable across CI runs. The canvas registers the same fonts with
//! gpui's text system for display.

use parking_lot::Mutex;
use std::sync::LazyLock;

use crate::element::{Element, ElementType};
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics};

/// Embedded font binaries (see assets/fonts/LICENSES.md for terms + sources).
pub static EXCALIFONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Excalifont-Regular.ttf"
));
pub static NUNITO: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Nunito-Regular.ttf"
));
pub static COMIC_SHANNS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/ComicShanns-Regular.ttf"
));

/// Excalidraw numeric `fontFamily` → family name we register everywhere.
pub fn family_for(font_family: i64) -> &'static str {
    match font_family {
        2 => "Nunito",
        3 => "Comic Shanns",
        _ => "Excalifont", // 1 (hand-drawn) and unknown → default
    }
}

/// All bundled fonts as (family name, bytes) — shared with the gpui layer.
pub fn bundled_fonts() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("Excalifont", EXCALIFONT),
        ("Nunito", NUNITO),
        ("Comic Shanns", COMIC_SHANNS),
    ]
}

fn build_font_system() -> FontSystem {
    let mut db = cosmic_text::fontdb::Database::new();
    for (_, bytes) in bundled_fonts() {
        db.load_font_data(bytes.to_vec());
    }
    // Deterministic: no system fonts, no fontconfig.
    FontSystem::new_with_locale_and_db("en".to_string(), db)
}

/// Shared process-wide engine (MCP updates + canvas share one shaping cache).
static GLOBAL_ENGINE: LazyLock<TextEngine> = LazyLock::new(TextEngine::bundled);

pub fn global_engine() -> &'static TextEngine {
    &GLOBAL_ENGINE
}

pub struct TextEngine {
    font_system: Mutex<FontSystem>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::bundled()
    }
}

impl TextEngine {
    pub fn bundled() -> Self {
        Self {
            font_system: Mutex::new(build_font_system()),
        }
    }

    fn shape(
        &self,
        text: &str,
        family: &str,
        font_size: f64,
        line_height: f64,
        wrap_width: Option<f32>,
    ) -> Vec<(f32, f32, String)> {
        // (line_width, line_height, line_text)
        let mut fs = self.font_system.lock();
        let metrics = Metrics {
            font_size: font_size as f32,
            line_height: (font_size * line_height) as f32,
        };
        let mut buffer = Buffer::new(&mut fs, metrics);
        buffer.set_size(&mut fs, wrap_width, None);
        buffer.set_text(
            &mut fs,
            text,
            &Attrs::new().family(cosmic_text::Family::Name(family)),
            cosmic_text::Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut fs, false);
        buffer
            .layout_runs()
            .map(|run| {
                // run.text is the LOGICAL line; the visual (wrapped) span is the
                // glyph index range within it.
                let (start, end) = match (run.glyphs.first(), run.glyphs.last()) {
                    (Some(first), Some(last)) => (first.start, last.end),
                    _ => (0, run.text.len()),
                };
                (
                    run.line_w,
                    run.line_height,
                    run.text[start..end].to_string(),
                )
            })
            .collect()
    }

    /// Measured (width, height) of `text` laid out unwrapped, in CSS pixels.
    pub fn measure(
        &self,
        text: &str,
        family: &str,
        font_size: f64,
        line_height: f64,
    ) -> (f64, f64) {
        if text.is_empty() {
            return (0.0, font_size * line_height);
        }
        let lines = self.shape(text, family, font_size, line_height, None);
        let w = lines.iter().map(|(w, _, _)| *w).fold(0.0f32, f32::max) as f64;
        let h = lines.iter().map(|(_, h, _)| *h).sum::<f32>() as f64;
        (w.ceil(), h.ceil())
    }

    /// Word-wrap `text` to `max_width`; returns the laid-out lines.
    pub fn wrap(
        &self,
        text: &str,
        family: &str,
        font_size: f64,
        line_height: f64,
        max_width: f64,
    ) -> Vec<String> {
        if max_width <= 0.0 {
            return text.split('\n').map(str::to_string).collect();
        }
        self.shape(text, family, font_size, line_height, Some(max_width as f32))
            .into_iter()
            .map(|(_, _, line)| line)
            .collect()
    }

    /// Recompute a text element's width/height from its content and style.
    ///
    /// - `autoResize == true` (default): unwrapped measurement.
    /// - `autoResize == false`: wrapped to the stored width; height grows.
    pub fn reflow(&self, el: &mut Element) {
        if el.kind != ElementType::Text || el.isDeleted {
            return;
        }
        let text = el.text.as_deref().unwrap_or("");
        let font_size = el.fontSize.unwrap_or(20.0);
        let lh = el.lineHeight.unwrap_or(1.25);
        let family = family_for(el.fontFamily.unwrap_or(1));
        if el.autoResize == Some(false) {
            let lines = self.wrap(text, family, font_size, lh, el.width);
            let w = lines
                .iter()
                .map(|l| self.measure(l, family, font_size, lh).0)
                .fold(0.0f64, f64::max);
            el.height = (lines.len() as f64 * font_size * lh).ceil();
            el.width = el.width.max(w);
        } else {
            let (w, h) = self.measure(text, family, font_size, lh);
            el.width = w.max(font_size); // keep a clickable box for empty text
            el.height = h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_nonzero_for_real_text() {
        let engine = TextEngine::bundled();
        let (w, h) = engine.measure("hello", "Excalifont", 20.0, 1.25);
        assert!(w > 20.0, "width {w}");
        assert!((h - 25.0).abs() < 1.0, "height {h} should be 20*1.25");
    }

    #[test]
    fn determinism_across_engines() {
        let a = TextEngine::bundled().measure("diagram", "Excalifont", 20.0, 1.25);
        let b = TextEngine::bundled().measure("diagram", "Excalifont", 20.0, 1.25);
        assert_eq!(a, b);
    }

    #[test]
    fn different_families_measure_differently() {
        let engine = TextEngine::bundled();
        let hand = engine.measure("mmm", "Excalifont", 20.0, 1.25).0;
        let mono = engine.measure("mmm", "Comic Shanns", 20.0, 1.25).0;
        assert!(hand > 0.0 && mono > 0.0);
        // Monospace 'mmm' should be exactly 3 advance widths; hand font differs.
        assert!(
            (hand - mono).abs() > 0.5,
            "fonts must have distinct metrics"
        );
    }

    #[test]
    fn wraps_long_text_to_max_width() {
        let engine = TextEngine::bundled();
        let text = "the quick brown fox jumps over the lazy dog again and again";
        let one_line = engine.measure(text, "Excalifont", 20.0, 1.25).0;
        let wrapped = engine.wrap(text, "Excalifont", 20.0, 1.25, one_line * 0.5);
        assert!(
            wrapped.len() >= 2,
            "should wrap into multiple lines: {wrapped:?}"
        );
        // Every laid-out line respects the limit (within rounding).
        for line in &wrapped {
            let lw = engine.measure(line, "Excalifont", 20.0, 1.25).0;
            assert!(lw <= one_line * 0.5 + 2.0, "line '{line}' too wide: {lw}");
        }
    }

    #[test]
    fn reflow_updates_box_for_autoresize_and_wrapped() {
        let engine = TextEngine::bundled();
        let mut auto = Element {
            kind: ElementType::Text,
            text: Some("hello world".into()),
            fontSize: Some(20.0),
            lineHeight: Some(1.25),
            autoResize: Some(true),
            ..Default::default()
        };
        engine.reflow(&mut auto);
        assert!(auto.width >= 60.0);
        assert_eq!(auto.height, 25.0);

        let mut wrapped = auto.clone();
        wrapped.autoResize = Some(false);
        wrapped.width = 40.0; // force narrow
        engine.reflow(&mut wrapped);
        assert!(
            wrapped.height > 25.0,
            "wrapped height must grow: {}",
            wrapped.height
        );
    }

    #[test]
    fn family_mapping_matches_excalidraw_ids() {
        assert_eq!(family_for(1), "Excalifont");
        assert_eq!(family_for(2), "Nunito");
        assert_eq!(family_for(3), "Comic Shanns");
        assert_eq!(family_for(0), "Excalifont");
        assert_eq!(family_for(99), "Excalifont");
    }
}
