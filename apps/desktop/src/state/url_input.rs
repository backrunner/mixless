//! UTF-8 byte selection for the single-line Spotify URL field.
#[derive(Default)]
pub struct UrlSelection {
    pub cursor: usize,
    pub anchor: Option<usize>,
}

impl UrlSelection {
    pub fn range(&self) -> std::ops::Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
    pub fn move_to(&mut self, cursor: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = cursor;
    }
    pub fn replace(&mut self, text: &mut String, replacement: &str) {
        let range = self.range();
        self.cursor = range.start + replacement.len();
        text.replace_range(range, replacement);
        self.anchor = None;
    }
    pub fn adjacent(&self, text: &str, right: bool) -> usize {
        if right {
            text[self.cursor..]
                .chars()
                .next()
                .map_or(self.cursor, |c| self.cursor + c.len_utf8())
        } else {
            text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editing_and_selection_preserve_utf8_and_cursor() {
        let mut text = "a曲b".to_string();
        let mut selection = UrlSelection::default();
        selection.move_to(1, false);
        selection.move_to(selection.adjacent(&text, true), true);
        selection.replace(&mut text, "https");
        assert_eq!(text, "ahttpsb");
        assert_eq!(selection.cursor, 6);
        selection.move_to(0, false);
        selection.move_to(text.len(), true);
        selection.replace(&mut text, "");
        assert!(text.is_empty());
        assert_eq!(selection.cursor, 0);
    }
}
