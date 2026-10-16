//! Cursor clamping property tests.
//!
//! The pane cursor is a hard invariant: when the text rect is non-empty,
//! the cursor cell is always inside it, even when the model cursor is
//! scrolled out of view. This test sweeps cursor positions against pane
//! edges across split and scrollbar configurations, verifying the
//! invariant holds.

use erax::core::geometry::{compute_frame, FrameOptions, WindowGeomInput};
use erax::core::id::WindowId;
use erax::core::layout::LayoutNode;

/// Build a single-pane layout and verify cursor clamping.
fn check_single_pane(
    cols: usize,
    rows: usize,
    cursor_y: usize,
    cursor_x: usize,
    scroll_offset: usize,
    scroll_x: usize,
) {
    let options = FrameOptions {
        cols,
        rows,
        menu_rows: 0,
        show_line_numbers: false,
    };
    let layout = LayoutNode::Leaf(WindowId(1));
    let input = WindowGeomInput {
        window: WindowId(1),
        scroll_offset,
        scroll_x,
        cursor_y,
        visual_cursor_x: cursor_x,
        line_count: 100,
        content_width: 100,
    };
    let frame = compute_frame(options, &layout, &[input]);
    let pane = frame.pane_for(WindowId(1)).expect("pane exists");
    let text = pane.text;

    if text.width == 0 || text.height == 0 {
        assert!(
            pane.cursor.is_none(),
            "empty text rect must have no cursor: {pane:?}"
        );
    } else {
        let (cx, cy) = pane.cursor.expect("non-empty text rect must have cursor");
        assert!(
            cx >= text.x && cx < text.x + text.width,
            "cursor x {cx} outside text rect x {}..{}",
            text.x,
            text.x + text.width
        );
        assert!(
            cy >= text.y && cy < text.y + text.height,
            "cursor y {cy} outside text rect y {}..{}",
            text.y,
            text.y + text.height
        );
    }
}

#[test]
fn cursor_clamps_to_text_rect_single_pane() {
    // Sweep cursor positions: inside, at edges, beyond edges.
    for cols in [20, 80] {
        for rows in [10, 24] {
            for cursor_y in [0, 5, 9, 10, 23, 24, 100] {
                for cursor_x in [0, 10, 19, 20, 79, 80, 200] {
                    for scroll_offset in [0, 5] {
                        for scroll_x in [0, 10] {
                            check_single_pane(
                                cols,
                                rows,
                                cursor_y,
                                cursor_x,
                                scroll_offset,
                                scroll_x,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn cursor_clamps_with_horizontal_split() {
    let options = FrameOptions {
        cols: 80,
        rows: 24,
        menu_rows: 0,
        show_line_numbers: true,
    };
    let layout = LayoutNode::HSplit {
        left: Box::new(LayoutNode::Leaf(WindowId(1))),
        right: Box::new(LayoutNode::Leaf(WindowId(2))),
        split_ratio: 0.5,
    };
    // Cursor far outside both panes; each must clamp to its own text rect.
    let inputs = [
        WindowGeomInput {
            window: WindowId(1),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 100,
            visual_cursor_x: 200,
            line_count: 100,
            content_width: 200,
        },
        WindowGeomInput {
            window: WindowId(2),
            scroll_offset: 50,
            scroll_x: 0,
            cursor_y: 0,
            visual_cursor_x: 0,
            line_count: 100,
            content_width: 200,
        },
    ];
    let frame = compute_frame(options, &layout, &inputs);
    for window in [WindowId(1), WindowId(2)] {
        let pane = frame.pane_for(window).expect("pane exists");
        let text = pane.text;
        if text.width > 0 && text.height > 0 {
            let (cx, cy) = pane.cursor.expect("cursor must exist");
            assert!(cx >= text.x && cx < text.x + text.width);
            assert!(cy >= text.y && cy < text.y + text.height);
        }
    }
}

#[test]
fn cursor_clamps_with_vertical_split_and_scrollbars() {
    let options = FrameOptions {
        cols: 80,
        rows: 24,
        menu_rows: 1,
        show_line_numbers: true,
    };
    let layout = LayoutNode::VSplit {
        top: Box::new(LayoutNode::Leaf(WindowId(1))),
        bottom: Box::new(LayoutNode::Leaf(WindowId(2))),
        split_ratio: 0.5,
    };
    // Bottom pane cursor on the last line; must not escape into the
    // horizontal scrollbar strip or status row.
    let inputs = [
        WindowGeomInput {
            window: WindowId(1),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 5,
            visual_cursor_x: 10,
            line_count: 100,
            content_width: 200,
        },
        WindowGeomInput {
            window: WindowId(2),
            scroll_offset: 0,
            scroll_x: 0,
            cursor_y: 50,
            visual_cursor_x: 75,
            line_count: 100,
            content_width: 200,
        },
    ];
    let frame = compute_frame(options, &layout, &inputs);
    let pane = frame.pane_for(WindowId(2)).expect("bottom pane exists");
    let text = pane.text;
    let (cx, cy) = pane.cursor.expect("cursor must exist");
    // The cursor must be strictly above the hscroll row and status row.
    assert!(
        cy < pane.hscroll_row,
        "cursor y {cy} escaped into hscroll row {}",
        pane.hscroll_row
    );
    assert!(
        cy < pane.status_row,
        "cursor y {cy} escaped into status row {}",
        pane.status_row
    );
    assert!(cx >= text.x && cx < text.x + text.width);
    assert!(cy >= text.y && cy < text.y + text.height);
}
