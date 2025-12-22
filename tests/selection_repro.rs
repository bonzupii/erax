use erax::core::app::EditorApp;
use erax::core::mouse::{MouseButton, MouseEvent, MouseHandler};
use erax::core::selection::SelectionMode;

#[test]
fn test_mouse_selection_divergence_repro() {
    let mut app = EditorApp::new();

    // Setup buffer content
    let buffer_id = {
        let window = app.active_window_ref().unwrap();
        window.buffer_id
    };

    {
        let buffer = app.buffers.get_mut(&buffer_id).unwrap();
        buffer.insert(0, "Line 1\nLine 2\nLine 3");
    }

    let window_id = app.active_window;

    // Ensure window dimensions are set so screen_to_buffer_pos works
    {
        let window = app.windows.get_mut(&window_id).unwrap();
        window.set_dimensions(80, 24);
    }

    let handler = MouseHandler::new();

    // 1. Simulate a mouse click at (0, 0)
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        handler.handle_event(&MouseEvent::Click(0, 0, MouseButton::Left), window, buffer);
    }

    // 2. Simulate a mouse drag to (5, 0) -> selects "Line 1" (5 chars)
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        handler.handle_event(
            &MouseEvent::Drag(0, 0, 5, 0, MouseButton::Left),
            window,
            buffer,
        );
    }

    let window = app.windows.get(&window_id).unwrap();

    // BUG REPRODUCTION:
    // SelectionManager should have a selection
    assert!(
        window.selection_manager.has_selection(),
        "SelectionManager should have selection"
    );
    let sel = window.selection_manager.get_selection().unwrap();
    assert_eq!(sel.start(), 0);
    assert_eq!(sel.end(), 5);

    // BUT: window.mark (used by renderer) is likely still None or outdated
    // because MouseHandler doesn't update it!
    assert!(
        window.mark.is_none(),
        "Window.mark should be None because MouseHandler doesn't update it (Divergence!)"
    );
}

#[test]
fn test_unicode_selection_repro() {
    let mut app = EditorApp::new();

    let buffer_id = {
        let window = app.active_window_ref().unwrap();
        window.buffer_id
    };

    {
        let buffer = app.buffers.get_mut(&buffer_id).unwrap();
        // "🦀" is 4 bytes.
        buffer.insert(0, "🦀🦀🦀");
    }

    let window_id = app.active_window;
    {
        let window = app.windows.get_mut(&window_id).unwrap();
        window.set_dimensions(80, 24);
    }

    let handler = MouseHandler::new();

    // Click on the second crab (visual col 2, row 0)
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        // Assume width 2 for crab
        handler.handle_event(&MouseEvent::Click(2, 0, MouseButton::Left), window, buffer);
    }

    let window = app.windows.get(&window_id).unwrap();
    let buffer = app.buffers.get(&buffer_id).unwrap();

    // If it works correctly, it should be at grapheme 1, byte 4.
    let byte_pos = window.get_byte_offset(buffer).unwrap();
    assert_eq!(byte_pos, 4, "Click on second emoji should be at byte 4");
}

#[test]
fn test_keyboard_selection_absence_repro() {
    let mut app = EditorApp::new();

    let buffer_id = {
        let window = app.active_window_ref().unwrap();
        window.buffer_id
    };

    {
        let buffer = app.buffers.get_mut(&buffer_id).unwrap();
        buffer.insert(0, "Hello World");
    }

    let window_id = app.active_window;
    {
        let window = app.windows.get_mut(&window_id).unwrap();
        window.set_dimensions(80, 24);
    }

    // Move forward 5 times
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        for _ in 0..5 {
            window.move_forward(buffer);
        }
    }

    let window = app.windows.get(&window_id).unwrap();

    // BUG REPRODUCTION:
    // Keyboard movement commands currently don't interact with SelectionManager at all.
    assert!(
        !window.selection_manager.has_selection(),
        "Keyboard movement should not start selection by default"
    );

    // Check if we can start a selection manually and if movement extends it (it SHOULD now)
    {
        // Use the Command structure directly to test the fix in movement.rs
        use erax::core::command::Command;
        use erax::core::commands::movement::ForwardChar;

        let window = app.windows.get_mut(&window_id).unwrap();
        window
            .selection_manager
            .start_selection(0, SelectionMode::Character);

        // Execute command
        let cmd = ForwardChar;
        cmd.execute(&mut app, 1);
    }

    let window = app.windows.get(&window_id).unwrap();
    // Check if selection extended
    let is_extended = window
        .selection_manager
        .primary
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    assert!(
        is_extended,
        "Keyboard movement SHOULD extend SelectionManager selection now"
    );

    let sel = window.selection_manager.get_selection().unwrap();
    assert_eq!(sel.start(), 0);
    assert!(sel.end() > 0, "Selection end should be > 0");
}

/// Test selection handling on empty lines
#[test]
fn test_empty_line_selection() {
    let mut app = EditorApp::new();

    let buffer_id = {
        let window = app.active_window_ref().unwrap();
        window.buffer_id
    };

    {
        let buffer = app.buffers.get_mut(&buffer_id).unwrap();
        // Content with empty line in middle
        buffer.insert(0, "Line 1\n\nLine 3");
    }

    let window_id = app.active_window;
    {
        let window = app.windows.get_mut(&window_id).unwrap();
        window.set_dimensions(80, 24);
    }

    let handler = MouseHandler::new();

    // Click on empty line (row 1) and drag to next line
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        handler.handle_event(&MouseEvent::Click(0, 1, MouseButton::Left), window, buffer);
    }
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        handler.handle_event(
            &MouseEvent::Drag(0, 1, 4, 2, MouseButton::Left),
            window,
            buffer,
        );
    }

    let window = app.windows.get(&window_id).unwrap();

    assert!(
        window.selection_manager.has_selection(),
        "Should have selection spanning empty line"
    );
    let sel = window.selection_manager.get_selection().unwrap();
    assert!(sel.end() > sel.start(), "Selection should span content");
}

/// Test selection at end of file
#[test]
fn test_end_of_file_selection() {
    let mut app = EditorApp::new();

    let buffer_id = {
        let window = app.active_window_ref().unwrap();
        window.buffer_id
    };

    {
        let buffer = app.buffers.get_mut(&buffer_id).unwrap();
        buffer.insert(0, "Short");
    }

    let window_id = app.active_window;
    {
        let window = app.windows.get_mut(&window_id).unwrap();
        window.set_dimensions(80, 24);
    }

    let handler = MouseHandler::new();

    // Click and drag past end of content
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        handler.handle_event(&MouseEvent::Click(3, 0, MouseButton::Left), window, buffer);
    }
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        // Drag past end (col 10, but content is only 5 chars)
        handler.handle_event(
            &MouseEvent::Drag(3, 0, 10, 0, MouseButton::Left),
            window,
            buffer,
        );
    }

    let window = app.windows.get(&window_id).unwrap();

    assert!(
        window.selection_manager.has_selection(),
        "Should have selection at end"
    );
    let sel = window.selection_manager.get_selection().unwrap();
    // Selection end should be clamped to buffer length
    assert!(
        sel.end() <= 5,
        "Selection end should be clamped to buffer length"
    );
}

/// Test rapid double and triple clicks
#[test]
fn test_rapid_click_handling() {
    let mut app = EditorApp::new();

    let buffer_id = {
        let window = app.active_window_ref().unwrap();
        window.buffer_id
    };

    {
        let buffer = app.buffers.get_mut(&buffer_id).unwrap();
        buffer.insert(0, "Hello World Test");
    }

    let window_id = app.active_window;
    {
        let window = app.windows.get_mut(&window_id).unwrap();
        window.set_dimensions(80, 24);
    }

    let handler = MouseHandler::new();

    // Double click on "World" (position 6)
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        handler.handle_event(&MouseEvent::DoubleClick(6, 0), window, buffer);
    }

    {
        let window = app.windows.get(&window_id).unwrap();
        assert!(
            window.selection_manager.has_selection(),
            "Double click should create word selection"
        );
        let sel = window.selection_manager.get_selection().unwrap();
        // Should select "World " (bytes 6-12, word selection includes trailing boundary)
        assert_eq!(sel.start(), 6, "Word selection start");
        assert_eq!(sel.end(), 12, "Word selection end");
    }

    // Triple click on same position
    {
        let buffer = app.buffers.get(&buffer_id).unwrap();
        let window = app.windows.get_mut(&window_id).unwrap();
        handler.handle_event(&MouseEvent::TripleClick(6, 0), window, buffer);
    }

    {
        let window = app.windows.get(&window_id).unwrap();
        assert!(
            window.selection_manager.has_selection(),
            "Triple click should create line selection"
        );
        let sel = window.selection_manager.get_selection().unwrap();
        // Should select entire line (bytes 0-16)
        assert_eq!(sel.start(), 0, "Line selection start");
        assert_eq!(sel.end(), 16, "Line selection end");
    }
}
