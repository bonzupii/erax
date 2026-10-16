//! Kill ring: bounded clipboard history for killed text.
use crate::config::editor::KILL_RING_MAX_BYTES;

/// Kill ring for storing cut/copied text
#[derive(Debug, Clone)]
pub struct KillRing {
    /// Ring buffer of killed text strings
    ring: Vec<String>,
    /// Maximum size of the ring
    max_size: usize,
    /// Current index in the ring (for yank-pop rotation)
    current_index: usize,
    /// Whether the last action was a kill (for appending)
    last_action_was_kill: bool,
}

impl KillRing {
    /// Create a new kill ring with default size (64)
    pub fn new() -> Self {
        Self::with_size(64)
    }

    /// Create a new kill ring with specified size
    pub fn with_size(max_size: usize) -> Self {
        Self {
            ring: Vec::with_capacity(max_size),
            max_size,
            current_index: 0,
            last_action_was_kill: false,
        }
    }

    /// Push text to the kill ring.
    /// If append is true, append to the current head instead of pushing new.
    /// Returns false when the text was rejected as oversized: the cap is
    /// the config constant, and the rejection is reported by the caller —
    /// an oversized kill is never silently dropped.
    pub fn push(&mut self, text: &str, append: bool) -> bool {
        if text.is_empty() {
            return true;
        }

        // Reject oversized entries to prevent OOM
        if text.len() > KILL_RING_MAX_BYTES {
            return false;
        }

        if append && self.last_action_was_kill && !self.ring.is_empty() {
            // Append to the most recent entry (but check combined size)
            if let Some(last) = self.ring.last_mut()
                && last.len() + text.len() <= KILL_RING_MAX_BYTES {
                    last.push_str(text);
                } else {
                    return false;
                }
        } else {
            // Push new entry
            if self.ring.len() >= self.max_size {
                self.ring.remove(0); // Remove oldest
            }
            self.ring.push(text.to_string());
            self.current_index = self.ring.len().saturating_sub(1);
        }
        self.last_action_was_kill = true;
        true
    }

    /// User-facing report for a kill the ring refused as oversized.
    /// `deleted` says whether the text was still removed from the buffer
    /// (kills) or left in place (copies).
    pub fn oversize_report(byte_len: usize, deleted: bool) -> String {
        format!(
            "Killed text is {} bytes, over the kill-ring entry limit of {} bytes: {}",
            byte_len,
            KILL_RING_MAX_BYTES,
            if deleted {
                "deleted but not saved for yank"
            } else {
                "not saved for yank"
            }
        )
    }

    /// Get the current text to yank
    pub fn peek(&self) -> Option<&String> {
        if self.ring.is_empty() {
            None
        } else {
            // If current_index is valid, use it. Otherwise last.
            // current_index should track the "yank point"
            // But usually peek just returns the top, unless we are rotating.
            // If we haven't rotated, it's the last item.
            // If we have rotated, it's at current_index.
            // However, standard behavior: push resets current_index to top.
            // rotate moves current_index.
            self.ring.get(self.current_index)
        }
    }

    /// Yank (paste) the current text from kill ring - alias for peek()
    pub fn yank(&self) -> Option<&String> {
        self.peek()
    }

    /// Rotate the kill ring (for yank-pop)
    pub fn rotate(&mut self) -> Option<&String> {
        if self.ring.is_empty() {
            return None;
        }

        if self.current_index == 0 {
            self.current_index = self.ring.len() - 1;
        } else {
            self.current_index -= 1;
        }

        self.ring.get(self.current_index)
    }

    /// Reset the "last action was kill" state
    pub fn reset_kill_state(&mut self) {
        self.last_action_was_kill = false;
        // Also reset rotation index to top
        if !self.ring.is_empty() {
            self.current_index = self.ring.len() - 1;
        }
    }
}

impl Default for KillRing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_peek() {
        let mut kr = KillRing::new();
        kr.push("hello", false);
        assert_eq!(kr.peek(), Some(&"hello".to_string()));

        kr.push("world", false);
        assert_eq!(kr.peek(), Some(&"world".to_string()));
    }

    #[test]
    fn oversized_push_is_rejected_not_silently_dropped() {
        // The cap is the config constant; a rejection reports false
        // so the caller can tell the user instead of claiming "killed".
        let mut kr = KillRing::new();
        assert!(kr.push("hello", false));
        let big = "x".repeat(KILL_RING_MAX_BYTES + 1);
        assert!(!kr.push(&big, false));
        // The rejected text never entered the ring.
        assert_eq!(kr.peek(), Some(&"hello".to_string()));
        let report = KillRing::oversize_report(big.len(), true);
        assert!(report.contains("not saved for yank"), "got: {report}");
    }

    #[test]
    fn test_append() {
        let mut kr = KillRing::new();
        kr.push("hello", false);
        kr.push(" world", true);
        assert_eq!(kr.peek(), Some(&"hello world".to_string()));
        assert_eq!(kr.ring.len(), 1);
    }

    #[test]
    fn test_append_broken_sequence() {
        let mut kr = KillRing::new();
        kr.push("hello", false);
        kr.reset_kill_state();
        kr.push(" world", true); // Should not append because state was reset
        // Wait, logic says: if append && last_action_was_kill.
        // If we pass append=true but last_action_was_kill is false, it behaves like push new?
        // The requirements say "Sequential kills append".
        // So if I call push(..., true) it implies the command *wants* to append.
        // But the KillRing checks internal state.

        // Let's verify implementation:
        // if append && self.last_action_was_kill
        // So if reset_kill_state was called, last_action_was_kill is false.
        // So it goes to else block -> push new.

        assert_eq!(kr.peek(), Some(&" world".to_string()));
        assert_eq!(kr.ring.len(), 2);
        assert_eq!(kr.ring[0], "hello");
    }

    #[test]
    fn test_rotate() {
        let mut kr = KillRing::new();
        kr.push("one", false);
        kr.push("two", false);
        kr.push("three", false);

        // Initial state: points to "three"
        assert_eq!(kr.peek(), Some(&"three".to_string()));

        // Rotate: should point to "two"
        assert_eq!(kr.rotate(), Some(&"two".to_string()));

        // Rotate: should point to "one"
        assert_eq!(kr.rotate(), Some(&"one".to_string()));

        // Rotate: should wrap to "three"
        assert_eq!(kr.rotate(), Some(&"three".to_string()));
    }

    #[test]
    fn test_max_size() {
        let mut kr = KillRing::with_size(2);
        kr.push("one", false);
        kr.push("two", false);
        kr.push("three", false);

        assert_eq!(kr.ring.len(), 2);
        assert_eq!(kr.ring[0], "two");
        assert_eq!(kr.ring[1], "three");
    }
}
