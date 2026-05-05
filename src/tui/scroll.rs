#[derive(Default)]
pub(crate) struct ScrollState {
    pub(crate) offset: u16,
    follow_tail: bool,
    viewport_height: u16,
    content_height: u16,
}

impl ScrollState {
    pub(crate) fn following_tail() -> Self {
        Self {
            follow_tail: true,
            ..Self::default()
        }
    }

    pub(crate) fn set_content_size(&mut self, viewport_height: u16, content_height: u16) {
        self.viewport_height = viewport_height;
        self.content_height = content_height;
        self.adjust_to_content();
    }

    pub(crate) fn content_changed(&mut self) {
        if self.follow_tail {
            self.scroll_to_bottom();
        }
    }

    pub(crate) fn follow_tail(&mut self) {
        self.follow_tail = true;
        self.scroll_to_bottom();
    }

    pub(crate) fn scroll_by(&mut self, delta: i16) {
        if delta < 0 {
            self.offset = self.offset.saturating_sub(delta.unsigned_abs());
            self.follow_tail = false;
        } else {
            self.offset = self.offset.saturating_add(delta as u16);
            self.clamp();
        }
    }

    fn adjust_to_content(&mut self) {
        if self.follow_tail {
            self.scroll_to_bottom();
        } else {
            self.clamp();
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.offset = self.max_offset();
        self.follow_tail = true;
    }

    fn clamp(&mut self) {
        let max = self.max_offset();
        self.offset = self.offset.min(max);
        if self.offset == max {
            self.follow_tail = true;
        }
    }

    fn max_offset(&self) -> u16 {
        self.content_height.saturating_sub(self.viewport_height)
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollState;

    #[test]
    fn follows_tail_as_content_grows() {
        let mut scroll = ScrollState::following_tail();

        scroll.set_content_size(10, 30);

        assert_eq!(scroll.offset, 20);
        assert!(scroll.follow_tail);
    }

    #[test]
    fn manual_scroll_disables_tail_following_until_bottom() {
        let mut scroll = ScrollState::following_tail();
        scroll.set_content_size(10, 30);

        scroll.scroll_by(-5);
        assert_eq!(scroll.offset, 15);
        assert!(!scroll.follow_tail);

        scroll.scroll_by(99);
        assert_eq!(scroll.offset, 20);
        assert!(scroll.follow_tail);
    }
}
