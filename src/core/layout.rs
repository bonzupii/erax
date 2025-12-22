use crate::core::id::WindowId;

/// Direction of a window split
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// A rectangle representing a screen area
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

// =============================================================================
// Viewport - Unified editor area calculation
// =============================================================================

/// Unified viewport area calculations
///
/// Provides consistent sizing for the editor area, accounting for:
/// - Menu bar at top
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    /// Editor area after menu subtraction
    pub editor: Rect,
}

impl Viewport {
    /// Calculate viewport from terminal size
    pub fn new(cols: u16, rows: u16, show_menu: bool) -> Self {
        let menu_height = if show_menu { 1 } else { 0 };

        let editor = Rect::new(
            0,
            menu_height,
            cols as usize,
            (rows as usize).saturating_sub(menu_height),
        );

        Self { editor }
    }
}

/// A node in the layout tree
#[derive(Debug, Clone)]
pub enum LayoutNode {
    /// A leaf node containing a window
    Leaf(WindowId),
    /// A horizontal split (left/right)
    HSplit {
        left: Box<LayoutNode>,
        right: Box<LayoutNode>,
        split_ratio: f32,
    },
    /// A vertical split (top/bottom)
    VSplit {
        top: Box<LayoutNode>,
        bottom: Box<LayoutNode>,
        split_ratio: f32,
    },
}

impl LayoutNode {
    /// Create a new leaf node
    pub fn new_leaf(window_id: WindowId) -> Self {
        Self::Leaf(window_id)
    }

    /// Collect all windows and their calculated rectangles
    pub fn collect_windows(&self, area: Rect) -> Vec<(WindowId, Rect)> {
        let mut windows = Vec::new();
        match self {
            Self::Leaf(id) => {
                windows.push((*id, area));
            }
            Self::HSplit {
                left,
                right,
                split_ratio,
            } => {
                let left_width = (area.width as f32 * split_ratio) as usize;
                let right_width = area.width - left_width;

                let left_rect = Rect::new(area.x, area.y, left_width, area.height);
                let right_rect = Rect::new(area.x + left_width, area.y, right_width, area.height);

                windows.extend(left.collect_windows(left_rect));
                windows.extend(right.collect_windows(right_rect));
            }
            Self::VSplit {
                top,
                bottom,
                split_ratio,
            } => {
                let top_height = (area.height as f32 * split_ratio) as usize;
                let bottom_height = area.height - top_height;

                let top_rect = Rect::new(area.x, area.y, area.width, top_height);
                let bottom_rect = Rect::new(area.x, area.y + top_height, area.width, bottom_height);

                windows.extend(top.collect_windows(top_rect));
                windows.extend(bottom.collect_windows(bottom_rect));
            }
        }
        windows
    }

    /// Remove a window from the layout tree
    /// Returns the ID of the window that should take focus (if any)
    pub fn remove_window(&mut self, target_id: WindowId) -> Option<WindowId> {
        self.remove_window_recursive(target_id)
    }

    fn remove_window_recursive(&mut self, target_id: WindowId) -> Option<WindowId> {
        match self {
            Self::Leaf(_) => None, // Cannot remove leaf directly without parent context
            Self::HSplit { left, right, .. } => {
                if let Self::Leaf(id) = **left {
                    if id == target_id {
                        // Remove left, replace self with right
                        let right_ids = right.window_ids();
                        let next_focus = right_ids.first().copied();
                        *self = *right.clone();
                        return next_focus;
                    }
                }
                if let Self::Leaf(id) = **right {
                    if id == target_id {
                        // Remove right, replace self with left
                        let left_ids = left.window_ids();
                        let next_focus = left_ids.last().copied();
                        *self = *left.clone();
                        return next_focus;
                    }
                }

                // Recurse
                if let Some(focus) = left.remove_window_recursive(target_id) {
                    return Some(focus);
                }
                right.remove_window_recursive(target_id)
            }
            Self::VSplit { top, bottom, .. } => {
                if let Self::Leaf(id) = **top {
                    if id == target_id {
                        // Remove top, replace self with bottom
                        let bottom_ids = bottom.window_ids();
                        let next_focus = bottom_ids.first().copied();
                        *self = *bottom.clone();
                        return next_focus;
                    }
                }
                if let Self::Leaf(id) = **bottom {
                    if id == target_id {
                        // Remove bottom, replace self with top
                        let top_ids = top.window_ids();
                        let next_focus = top_ids.last().copied();
                        *self = *top.clone();
                        return next_focus;
                    }
                }

                // Recurse
                if let Some(focus) = top.remove_window_recursive(target_id) {
                    return Some(focus);
                }
                bottom.remove_window_recursive(target_id)
            }
        }
    }

    /// Get all window IDs in this subtree
    pub fn window_ids(&self) -> Vec<WindowId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::HSplit { left, right, .. } => {
                let mut ids = left.window_ids();
                ids.extend(right.window_ids());
                ids
            }
            Self::VSplit { top, bottom, .. } => {
                let mut ids = top.window_ids();
                ids.extend(bottom.window_ids());
                ids
            }
        }
    }

    /// Split the window with the given ID
    pub fn split_window(
        &mut self,
        target_id: WindowId,
        new_id: WindowId,
        direction: SplitDirection,
    ) -> bool {
        match self {
            Self::Leaf(id) => {
                if *id == target_id {
                    let old_leaf = Box::new(Self::Leaf(*id));
                    let new_leaf = Box::new(Self::Leaf(new_id));

                    *self = match direction {
                        SplitDirection::Horizontal => Self::HSplit {
                            left: old_leaf,
                            right: new_leaf,
                            split_ratio: 0.5,
                        },
                        SplitDirection::Vertical => Self::VSplit {
                            top: old_leaf,
                            bottom: new_leaf,
                            split_ratio: 0.5,
                        },
                    };
                    return true;
                }
                false
            }
            Self::HSplit { left, right, .. } => {
                left.split_window(target_id, new_id, direction)
                    || right.split_window(target_id, new_id, direction)
            }
            Self::VSplit { top, bottom, .. } => {
                top.split_window(target_id, new_id, direction)
                    || bottom.split_window(target_id, new_id, direction)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_remove_and_focus() {
        // Create a single leaf layout
        let mut layout = LayoutNode::new_leaf(WindowId(0));

        // Verify initial state
        assert_eq!(layout.window_ids(), vec![WindowId(0)]);

        // Split it vertically to create two windows
        assert!(layout.split_window(WindowId(0), WindowId(1), SplitDirection::Vertical));
        assert_eq!(layout.window_ids(), vec![WindowId(0), WindowId(1)]);

        // Further split one of the leaves horizontally
        assert!(layout.split_window(WindowId(0), WindowId(2), SplitDirection::Horizontal));
        assert_eq!(
            layout.window_ids(),
            vec![WindowId(0), WindowId(2), WindowId(1)]
        );

        // Remove the window (WindowId(2))
        let next_focus = layout.remove_window(WindowId(2));

        // Check that the returned focus is correct (should be one of the remaining windows)
        // Either WindowId(0) or WindowId(1) depending on internal logic
        assert!(next_focus.is_some());

        // Check that the final layout contains the correct remaining window IDs
        let remaining_ids = layout.window_ids();
        assert!(remaining_ids.contains(&WindowId(0)));
        assert!(remaining_ids.contains(&WindowId(1)));
        assert!(!remaining_ids.contains(&WindowId(2))); // Window 2 should be removed
    }
}
