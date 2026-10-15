//! Window layout: tiling geometry for splits.
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

/// How much one grow/shrink step moves a split ratio. A 0.05 step takes
/// ten invocations to cross half the screen; the ratio is clamped to
/// [0.1, 0.9] so no pane can be squeezed out of existence.
pub const SPLIT_ADJUST_STEP: f32 = 0.05;

impl LayoutNode {
    /// Create a new leaf node
    pub fn new_leaf(window_id: WindowId) -> Self {
        Self::Leaf(window_id)
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
                if let Self::Leaf(id) = **left
                    && id == target_id {
                        // Remove left, replace self with right
                        let right_ids = right.window_ids();
                        let next_focus = right_ids.first().copied();
                        *self = *right.clone();
                        return next_focus;
                    }
                if let Self::Leaf(id) = **right
                    && id == target_id {
                        // Remove right, replace self with left
                        let left_ids = left.window_ids();
                        let next_focus = left_ids.last().copied();
                        *self = *left.clone();
                        return next_focus;
                    }

                // Recurse
                if let Some(focus) = left.remove_window_recursive(target_id) {
                    return Some(focus);
                }
                right.remove_window_recursive(target_id)
            }
            Self::VSplit { top, bottom, .. } => {
                if let Self::Leaf(id) = **top
                    && id == target_id {
                        // Remove top, replace self with bottom
                        let bottom_ids = bottom.window_ids();
                        let next_focus = bottom_ids.first().copied();
                        *self = *bottom.clone();
                        return next_focus;
                    }
                if let Self::Leaf(id) = **bottom
                    && id == target_id {
                        // Remove bottom, replace self with top
                        let top_ids = top.window_ids();
                        let next_focus = top_ids.last().copied();
                        *self = *top.clone();
                        return next_focus;
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

    /// Grow the pane holding `target_id` by `delta`, shrinking its
    /// sibling. The innermost split containing the window adjusts; the
    /// ratio clamps to [0.1, 0.9]. Returns false when the window is not
    /// in any split (a lone window has nothing to grow against).
    pub fn adjust_split(&mut self, target_id: WindowId, delta: f32) -> bool {
        match self {
            Self::Leaf(_) => false,
            Self::HSplit {
                left,
                right,
                split_ratio,
            } => {
                if left.adjust_split(target_id, delta) || right.adjust_split(target_id, delta) {
                    return true;
                }
                if left.window_ids().contains(&target_id) {
                    *split_ratio = (*split_ratio + delta).clamp(0.1, 0.9);
                    true
                } else if right.window_ids().contains(&target_id) {
                    *split_ratio = (*split_ratio - delta).clamp(0.1, 0.9);
                    true
                } else {
                    false
                }
            }
            Self::VSplit {
                top,
                bottom,
                split_ratio,
            } => {
                if top.adjust_split(target_id, delta) || bottom.adjust_split(target_id, delta) {
                    return true;
                }
                if top.window_ids().contains(&target_id) {
                    *split_ratio = (*split_ratio + delta).clamp(0.1, 0.9);
                    true
                } else if bottom.window_ids().contains(&target_id) {
                    *split_ratio = (*split_ratio - delta).clamp(0.1, 0.9);
                    true
                } else {
                    false
                }
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

    /// Pane frames from the unified model for these layout tests.
    /// The tiling rule lives in core::geometry; these tests assert the
    /// layout tree drives it correctly.
    fn model_frames(layout: &LayoutNode) -> Vec<(WindowId, Rect)> {
        use crate::core::geometry::{FrameOptions, WindowGeomInput, compute_frame};
        let inputs: Vec<WindowGeomInput> = layout
            .window_ids()
            .iter()
            .map(|id| WindowGeomInput {
                window: *id,
                scroll_offset: 0,
                scroll_x: 0,
                cursor_y: 0,
                visual_cursor_x: 0,
                line_count: 0,
                content_width: 0,
            })
            .collect();
        compute_frame(
            FrameOptions {
                cols: 80,
                rows: 40,
                menu_rows: 0,
                show_line_numbers: false,
            },
            layout,
            &inputs,
        )
        .panes
        .into_iter()
        .map(|p| (p.window, p.frame))
        .collect()
    }

    #[test]
    fn test_adjust_split_grows_active_pane() {
        let mut layout = LayoutNode::new_leaf(WindowId(0));
        assert!(layout.split_window(WindowId(0), WindowId(1), SplitDirection::Vertical));
        let height_of = |layout: &LayoutNode, id: WindowId| {
            model_frames(layout)
                .into_iter()
                .find(|(wid, _)| *wid == id)
                .map(|(_, r)| r.height)
                .unwrap()
        };
        let before = height_of(&layout, WindowId(0));
        assert!(layout.adjust_split(WindowId(0), SPLIT_ADJUST_STEP));
        let after = height_of(&layout, WindowId(0));
        assert!(after > before, "growing window 0 must enlarge its pane");
        assert!(layout.adjust_split(WindowId(0), -SPLIT_ADJUST_STEP));
        assert_eq!(height_of(&layout, WindowId(0)), before);
    }

    #[test]
    fn test_adjust_split_clamps_and_ignores_lone_window() {
        let mut layout = LayoutNode::new_leaf(WindowId(0));
        assert!(!layout.adjust_split(WindowId(0), SPLIT_ADJUST_STEP));
        assert!(layout.split_window(WindowId(0), WindowId(1), SplitDirection::Horizontal));
        for _ in 0..40 {
            layout.adjust_split(WindowId(0), SPLIT_ADJUST_STEP);
        }
        // The ratio clamps: window 1's pane never vanishes entirely.
        let right_width = model_frames(&layout)
            .into_iter()
            .find(|(wid, _)| *wid == WindowId(1))
            .map(|(_, r)| r.width)
            .unwrap();
        assert!(right_width >= 8, "clamp keeps the sibling pane visible");
    }
}
