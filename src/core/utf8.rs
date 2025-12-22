use unicode_segmentation::UnicodeSegmentation;

/// Iterator over grapheme clusters in a string slice.
///
/// This handles complex Unicode characters (like emoji with modifiers, or combined characters)
/// as single logical units, which is essential for a text editor.
pub struct GraphemeIterator<'a> {
    iter: unicode_segmentation::Graphemes<'a>,
}

impl<'a> GraphemeIterator<'a> {
    /// Create a new GraphemeIterator from a string slice
    pub fn new(text: &'a str) -> Self {
        Self {
            iter: text.graphemes(true),
        }
    }
}

impl<'a> Iterator for GraphemeIterator<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}

impl<'a> DoubleEndedIterator for GraphemeIterator<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iter.next_back()
    }
}

/// Helper to count grapheme clusters in a string
pub fn grapheme_count(text: &str) -> usize {
    text.graphemes(true).count()
}

/// Helper to get the byte index of the nth grapheme cluster (grapheme → byte)
pub fn grapheme_byte_index(text: &str, n: usize) -> Option<usize> {
    text.grapheme_indices(true).nth(n).map(|(idx, _)| idx)
}

/// Helper to get the grapheme column at or before a byte offset (byte → grapheme)
/// This is the inverse of grapheme_byte_index
pub fn byte_to_grapheme_col(text: &str, byte_offset: usize) -> usize {
    let mut col = 0;
    for (idx, _) in text.grapheme_indices(true) {
        if idx >= byte_offset {
            break;
        }
        col += 1;
    }
    col
}

/// Get the display width of a single character (for monospace terminal display)
/// Returns 0 for control characters, 1 for ASCII, 2 for wide CJK characters, etc.
pub fn char_width(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Get the display width of a grapheme cluster (sum of all character widths)
pub fn grapheme_width(g: &str) -> usize {
    g.chars().map(char_width).sum()
}

/// Get the visual (display) width of text up to a grapheme index, with tab handling
/// Used for cursor positioning in terminals where tabs expand to multiple columns
pub fn visual_width_up_to(text: &str, grapheme_idx: usize, tab_width: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    let mut visual_x = 0;
    for (i, grapheme) in text.graphemes(true).enumerate() {
        if i >= grapheme_idx {
            break;
        }
        if grapheme == "\t" {
            visual_x = (visual_x / tab_width + 1) * tab_width;
        } else {
            visual_x += grapheme_width(grapheme);
        }
    }
    visual_x
}

/// Get the total visual (display) width of text, with tab handling
/// Used for determining if horizontal scrollbar is needed
pub fn visual_width(text: &str, tab_width: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    let mut visual_x = 0;
    for grapheme in text.graphemes(true) {
        if grapheme == "\t" {
            visual_x = (visual_x / tab_width + 1) * tab_width;
        } else {
            visual_x += grapheme_width(grapheme);
        }
    }
    visual_x
}

/// Find the grapheme index corresponding to a visual x position
/// This is the inverse of `visual_width_up_to`, used for mouse clicking
pub fn grapheme_index_from_visual_x(text: &str, target_visual_x: usize, tab_width: usize) -> usize {
    use unicode_segmentation::UnicodeSegmentation;
    let mut current_visual_x = 0;

    for (i, grapheme) in text.graphemes(true).enumerate() {
        // Calculate width of this grapheme
        let width = if grapheme == "\t" {
            let next_tab_stop = (current_visual_x / tab_width + 1) * tab_width;
            next_tab_stop - current_visual_x
        } else {
            grapheme_width(grapheme)
        };

        // If clicking on the left half of a wide char, picking 'i' is correct.
        // If clicking past the middle, we might prefer 'i+1', but standard behavior
        // is usually "start of character".
        // If the click is *within* this character's width:
        if current_visual_x + width > target_visual_x {
            return i;
        }

        current_visual_x += width;
    }

    // If we ran out of text, return the length (index after last char)
    grapheme_count(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_graphemes() {
        let text = "Hello";
        let graphemes: Vec<&str> = GraphemeIterator::new(text).collect();
        assert_eq!(graphemes, vec!["H", "e", "l", "l", "o"]);
        assert_eq!(grapheme_count(text), 5);
    }

    #[test]
    fn test_emoji_graphemes() {
        let text = "👋🌍";
        let graphemes: Vec<&str> = GraphemeIterator::new(text).collect();
        assert_eq!(graphemes, vec!["👋", "🌍"]);
        assert_eq!(grapheme_count(text), 2);
    }

    #[test]
    fn test_combining_characters() {
        // "e" + acute accent
        let text = "e\u{0301}";
        let graphemes: Vec<&str> = GraphemeIterator::new(text).collect();
        // Should be treated as one grapheme
        assert_eq!(graphemes, vec!["e\u{0301}"]);
        assert_eq!(grapheme_count(text), 1);
    }

    #[test]
    fn test_zwj_sequences() {
        // Family emoji: Man + ZWJ + Woman + ZWJ + Boy
        let text = "👨‍👩‍👦";
        let graphemes: Vec<&str> = GraphemeIterator::new(text).collect();
        assert_eq!(graphemes, vec!["👨‍👩‍👦"]);
        assert_eq!(grapheme_count(text), 1);
    }

    #[test]
    fn test_grapheme_byte_index() {
        let text = "A👋B";
        // 'A' is 1 byte (idx 0)
        // '👋' is 4 bytes (idx 1)
        // 'B' is 1 byte (idx 5)

        assert_eq!(grapheme_byte_index(text, 0), Some(0));
        assert_eq!(grapheme_byte_index(text, 1), Some(1));
        assert_eq!(grapheme_byte_index(text, 2), Some(5));
        assert_eq!(grapheme_byte_index(text, 3), None);
    }

    #[test]
    fn test_reverse_iteration() {
        let text = "ABC";
        let mut iter = GraphemeIterator::new(text);
        assert_eq!(iter.next_back(), Some("C"));
        assert_eq!(iter.next_back(), Some("B"));
        assert_eq!(iter.next_back(), Some("A"));
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn test_grapheme_index_from_visual_x() {
        // "a\tb" with tab width 4
        // 'a': width 1, range [0, 1) -> index 0
        // '\t': width 3 (aligns to 4), range [1, 4) -> index 1
        // 'b': width 1, range [4, 5) -> index 2
        let text = "a\tb";
        assert_eq!(grapheme_index_from_visual_x(text, 0, 4), 0); // 'a'
        assert_eq!(grapheme_index_from_visual_x(text, 1, 4), 1); // '\t' start
        assert_eq!(grapheme_index_from_visual_x(text, 3, 4), 1); // '\t' end
        assert_eq!(grapheme_index_from_visual_x(text, 4, 4), 2); // 'b'

        // Wide char "👋" (width 2)
        let text_wide = "A👋B";
        assert_eq!(grapheme_index_from_visual_x(text_wide, 0, 4), 0); // 'A'
        assert_eq!(grapheme_index_from_visual_x(text_wide, 1, 4), 1); // '👋'
        assert_eq!(grapheme_index_from_visual_x(text_wide, 2, 4), 1); // '👋' (second half)
        assert_eq!(grapheme_index_from_visual_x(text_wide, 3, 4), 2); // 'B'
    }
}
