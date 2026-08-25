//! Inline text editing state: caret, insertion, deletion, commit.
//!
//! Toolkit-free so the editing contract is unit-testable; the canvas routes
//! keystrokes here while a text element is in edit mode and renders the caret.

/// Editing session for one text element. `working` holds the in-progress text;
/// `commit` produces the final string (the scene mutation happens in the caller).
#[derive(Debug, Clone, PartialEq)]
pub struct TextEditState {
    pub element_id: String,
    pub working: String,
    /// Byte offset into `working` (always on a UTF-8 char boundary).
    pub caret: usize,
}

impl TextEditState {
    pub fn new(element_id: impl Into<String>, initial: impl Into<String>) -> Self {
        let working = initial.into();
        Self {
            element_id: element_id.into(),
            caret: working.len(),
            working,
        }
    }

    pub fn text(&self) -> &str {
        &self.working
    }

    fn clamp_caret(&mut self) {
        while self.caret > self.working.len() && !self.working.is_char_boundary(self.caret) {
            self.caret += 1;
        }
        self.caret = self.caret.min(self.working.len());
    }

    /// Printable input: insert at caret.
    pub fn input(&mut self, s: &str) {
        if s.is_empty() || s.chars().any(|c| c.is_control()) {
            return;
        }
        self.working.insert_str(self.caret, s);
        self.caret += s.len();
    }

    /// Backspace: delete the char before the caret.
    pub fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        self.clamp_caret();
        let prev = self.working[..self.caret]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.working.replace_range(prev..self.caret, "");
        self.caret = prev;
    }

    /// Delete the char after the caret.
    pub fn delete(&mut self) {
        self.clamp_caret();
        if self.caret >= self.working.len() {
            return;
        }
        let next = self.working[self.caret..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.caret + i)
            .unwrap_or(self.working.len());
        self.working.replace_range(self.caret..next, "");
    }

    pub fn caret_left(&mut self) {
        self.clamp_caret();
        if let Some(prev) = self.working[..self.caret].char_indices().next_back().map(|(i, _)| i) {
            self.caret = prev;
        }
    }

    pub fn caret_right(&mut self) {
        self.clamp_caret();
        if let Some(next) = self.working[self.caret..].char_indices().nth(1).map(|(i, _)| self.caret + i) {
            self.caret = next;
        }
    }

    pub fn caret_home(&mut self) {
        let line_start = self.working[..self.caret].rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.caret = line_start;
    }

    pub fn caret_end(&mut self) {
        let line_end = self.working[self.caret..].find('\n').map(|i| self.caret + i).unwrap_or(self.working.len());
        self.caret = line_end;
    }

    /// Newline at caret (multi-line text elements).
    pub fn newline(&mut self) {
        self.working.insert(self.caret, '\n');
        self.caret += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_inserts_at_caret() {
        let mut ed = TextEditState::new("t", "hello");
        ed.caret_home();
        ed.input("X");
        assert_eq!(ed.text(), "Xhello");
        assert_eq!(ed.caret, 1);
        ed.caret_end();
        ed.input("!");
        assert_eq!(ed.text(), "Xhello!");
    }

    #[test]
    fn backspace_deletes_before_caret() {
        let mut ed = TextEditState::new("t", "abc");
        ed.caret_end();
        ed.backspace();
        assert_eq!(ed.text(), "ab");
        assert_eq!(ed.caret, 2);
        ed.backspace();
        ed.backspace();
        ed.backspace(); // no-op at start
        assert_eq!(ed.text(), "");
        assert_eq!(ed.caret, 0);
    }

    #[test]
    fn delete_removes_after_caret() {
        let mut ed = TextEditState::new("t", "abc");
        ed.caret_home();
        ed.delete();
        assert_eq!(ed.text(), "bc");
    }

    #[test]
    fn utf8_boundaries_respected() {
        let mut ed = TextEditState::new("t", "héllo"); // é is 2 bytes
        ed.caret_home();
        ed.caret_right(); // past 'h' → byte 1 (start of é, a boundary)
        assert_eq!(ed.caret, 1);
        ed.caret_right(); // past é → byte 3
        assert_eq!(ed.caret, 3);
        ed.backspace(); // deletes é whole; caret lands where é started
        assert_eq!(ed.text(), "hllo");
        assert_eq!(ed.caret, 1);
        ed.input("é"); // inserts at caret
        assert_eq!(ed.text(), "héllo");
        assert_eq!(ed.caret, 3);
    }

    #[test]
    fn control_chars_ignored() {
        let mut ed = TextEditState::new("t", "a");
        ed.input("\u{1}[B"); // escape sequence garbage
        assert_eq!(ed.text(), "a");
    }

    #[test]
    fn newlines_and_line_navigation() {
        // "ab\ncd": bytes a0 b1 \n2 c3 d4; fresh caret sits at the end (5).
        let mut ed = TextEditState::new("t", "ab\ncd");
        ed.caret_home(); // home of the CURRENT line ("cd") → 3
        assert_eq!(ed.caret, 3);
        ed.caret_end(); // end of that line → 5
        assert_eq!(ed.caret, 5);
        ed.caret_left(); // onto 'c'
        assert_eq!(ed.caret, 4);
        ed.caret_left(); // start of 'c' = end of first line
        assert_eq!(ed.caret, 3);
        ed.caret_left(); // onto the newline byte
        assert_eq!(ed.caret, 2);
        ed.caret_home(); // start of first line
        assert_eq!(ed.caret, 0);
        ed.newline(); // newline at caret 0 → text starts with a blank line
        assert_eq!(ed.text(), "\nab\ncd");
        assert_eq!(ed.caret, 1);
    }

}
