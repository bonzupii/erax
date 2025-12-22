/// ID-based handle system for safe ownership management
/// Replaces pointer-based linked lists with Rust-idiomatic handles
use std::fmt;

/// Unique identifier for a buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BufferId(pub usize);

impl fmt::Display for BufferId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Buffer({})", self.0)
    }
}

/// Unique identifier for a window (view into a buffer)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub usize);

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Window({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_id() {
        let id1 = BufferId(0);
        let id2 = BufferId(1);
        assert_ne!(id1, id2);
        assert_eq!(id1.0, 0);
        assert_eq!(format!("{}", id1), "Buffer(0)");
    }

    #[test]
    fn test_window_id() {
        let id1 = WindowId(0);
        let id2 = WindowId(1);
        assert_ne!(id1, id2);
        assert_eq!(id1.0, 0);
        assert_eq!(format!("{}", id1), "Window(0)");
    }

    #[test]
    fn test_id_hashable() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        let id = BufferId(42);
        map.insert(id, "test");
        assert_eq!(map.get(&id), Some(&"test"));
    }
}
