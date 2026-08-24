//! Element vocabulary mirrored from Excalidraw's schema
//! (`packages/element/src/types.ts`, master @ 2026-08).
//!
//! The full typed model (serde schema, restore semantics, bindings) lands in M1;
//! this pins the type discriminants early because everything else keys off them.

/// Every element `type` discriminator in the Excalidraw scene format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementType {
    Selection,
    Rectangle,
    Diamond,
    Ellipse,
    Embeddable,
    Iframe,
    Text,
    Line,
    Arrow,
    Freedraw,
    Image,
    Frame,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_match_excalidraw_schema() {
        let all = [
            (ElementType::Selection, "selection"),
            (ElementType::Rectangle, "rectangle"),
            (ElementType::Diamond, "diamond"),
            (ElementType::Ellipse, "ellipse"),
            (ElementType::Embeddable, "embeddable"),
            (ElementType::Iframe, "iframe"),
            (ElementType::Text, "text"),
            (ElementType::Line, "line"),
            (ElementType::Arrow, "arrow"),
            (ElementType::Freedraw, "freedraw"),
            (ElementType::Image, "image"),
            (ElementType::Frame, "frame"),
            (ElementType::MagicFrame, "magicframe"),
        ];
        for (ty, name) in all {
            assert_eq!(ty.as_str(), name);
        }
    }
}
