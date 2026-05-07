#[derive(Default)]
pub(crate) struct InputBuffer {
    text: String,
    cursor: usize,
}

impl InputBuffer {
    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub(crate) fn cursor_column(&self) -> u16 {
        self.text[..self.cursor].chars().count() as u16
    }

    pub(crate) fn clear(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub(crate) fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub(crate) fn insert_str(&mut self, s: &str) {
        let cleaned: String = s
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        self.text.insert_str(self.cursor, &cleaned);
        self.cursor += cleaned.len();
    }

    pub(crate) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.move_left();
        self.text.remove(self.cursor);
    }

    pub(crate) fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        while !self.text.is_char_boundary(self.cursor) {
            self.cursor -= 1;
        }
    }

    pub(crate) fn move_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        self.cursor += 1;
        while !self.text.is_char_boundary(self.cursor) {
            self.cursor += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InputBuffer;

    #[test]
    fn edits_across_utf8_boundaries() {
        let mut input = InputBuffer::default();

        input.insert_str("aé");
        input.move_left();
        input.backspace();
        input.insert_char('z');

        assert_eq!(input.as_str(), "zé");
        assert_eq!(input.cursor_column(), 1);
    }

    #[test]
    fn paste_replaces_controls_with_spaces() {
        let mut input = InputBuffer::default();

        input.insert_str("one\ntwo\tthree");

        assert_eq!(input.as_str(), "one two three");
    }

}
