//! Foundation Stress Tests
//!
//! Phase 2: Verify core data structures are rock-solid before building features on top.
//! These tests ensure buffer integrity, undo/redo, and UTF-8 handling are bulletproof.

use erax::core::app::EditorApp;
use erax::core::buffer::Buffer;

/// Simple hash function for content comparison (no crypto needed)
fn hash_content(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// =============================================================================
// BUFFER INTEGRITY TESTS
// =============================================================================

#[test]
fn buffer_10k_edits_undo_all() {
    // Create a buffer with initial content
    let initial_content = "Hello, World!\nThis is a test file.\nWith multiple lines.\n";
    let mut buffer = Buffer::from_string(initial_content);
    let original_hash = hash_content(&buffer.to_string());

    // Perform 10,000 single-char edits at end (adjacent = groups together)
    // With batch grouping (100 per group) this creates ~100 undo groups
    // Well within MAX_UNDO_DEPTH of 10,000
    for _ in 0..10_000u64 {
        buffer.insert(buffer.len(), "x");
    }

    // Buffer should be modified
    assert!(
        buffer.to_string() != initial_content,
        "Buffer should be modified after edits"
    );

    // Undo ALL edits - with 100 edits per group, ~100 undos needed
    let mut undo_count = 0;
    while buffer.undo() {
        undo_count += 1;
        if undo_count > 500 {
            panic!(
                "Too many undo operations: {} - batch grouping may not be working",
                undo_count
            );
        }
    }

    // Verify content matches original
    let final_hash = hash_content(&buffer.to_string());
    assert_eq!(
        original_hash,
        final_hash,
        "Data corruption detected! Original and final content differ after undo.\nOriginal: {:?}\nFinal: {:?}",
        initial_content,
        buffer.to_string()
    );
}

#[test]
fn buffer_deep_undo_redo_cycle() {
    let mut buffer = Buffer::from_string("Start\n");

    // 1000 inserts
    for i in 0..1000 {
        buffer.insert(buffer.len(), &format!("Line {}\n", i));
    }

    // Undo half
    for _ in 0..500 {
        assert!(buffer.undo(), "Undo should succeed");
    }

    // Redo all
    for _ in 0..500 {
        assert!(buffer.redo(), "Redo should succeed");
    }

    // Undo all
    while buffer.undo() {}

    // Should be back to original
    assert_eq!(buffer.to_string(), "Start\n");
}

#[test]
fn buffer_rapid_insert_delete_cycle() {
    let mut buffer = Buffer::new();

    // Rapid insert/delete cycles
    for round in 0..100 {
        // Insert 100 chars
        for _ in 0..100 {
            buffer.insert(0, "x");
        }

        // Delete 100 chars
        for _ in 0..100 {
            buffer.delete(0, 1);
        }

        // Buffer should be empty
        assert!(
            buffer.to_string().is_empty(),
            "Buffer should be empty after round {}, got: {:?}",
            round,
            buffer.to_string()
        );
    }
}

// =============================================================================
// UTF-8 BOUNDARY TESTS
// =============================================================================

#[test]
fn utf8_multibyte_delete() {
    let mut buffer = Buffer::from_string("Hello 世界 🦀");

    // Get byte position of 世 (after "Hello ")
    let world_start = "Hello ".len();

    // Delete 世 (3 bytes in UTF-8)
    buffer.delete(world_start, "世".len());

    assert_eq!(buffer.to_string(), "Hello 界 🦀");
}

#[test]
fn utf8_emoji_handling() {
    let mut buffer = Buffer::from_string("🦀🦀🦀");

    // Each crab emoji is 4 bytes
    let crab_len = "🦀".len();
    assert_eq!(crab_len, 4);

    // Delete middle crab
    buffer.delete(crab_len, crab_len);

    assert_eq!(buffer.to_string(), "🦀🦀");
}

#[test]
fn utf8_insert_at_multibyte_boundary() {
    let mut buffer = Buffer::from_string("日本語");

    // Insert between 日 and 本
    let insert_pos = "日".len();
    buffer.insert(insert_pos, "X");

    assert_eq!(buffer.to_string(), "日X本語");
}

#[test]
fn utf8_mixed_content_stress() {
    let mixed = "ASCII日本語🦀More ASCII한글";
    let mut buffer = Buffer::from_string(mixed);

    // Insert at various points
    buffer.insert(0, "→");
    buffer.insert(buffer.len(), "←");

    // Undo both
    buffer.undo();
    buffer.undo();

    assert_eq!(buffer.to_string(), mixed);
}

// =============================================================================
// LINE OPERATIONS
// =============================================================================

#[test]
fn line_indexing_stress() {
    let mut buffer = Buffer::new();

    // Build a large buffer
    for i in 0..1000 {
        buffer.insert(buffer.len(), &format!("Line {}\n", i));
    }

    // Verify line count
    assert_eq!(buffer.line_count(), 1001); // 1000 lines + trailing empty

    // Random access every line
    for i in 0..1000 {
        let line = buffer.line(i).expect(&format!("Line {} should exist", i));
        assert!(
            line.starts_with("Line "),
            "Line {} corrupted: {:?}",
            i,
            line
        );
    }
}

#[test]
fn line_to_byte_consistency() {
    let content = "Line 1\nLine 2\nLine 3\n";
    let buffer = Buffer::from_string(content);

    // Verify line_to_byte returns correct positions
    assert_eq!(buffer.line_to_byte(0), Some(0));
    assert_eq!(buffer.line_to_byte(1), Some(7)); // After "Line 1\n"
    assert_eq!(buffer.line_to_byte(2), Some(14)); // After "Line 2\n"
}

// =============================================================================
// MEMORY AND BOUNDS
// =============================================================================

#[test]
fn empty_buffer_operations() {
    let mut buffer = Buffer::new();

    // These should not panic
    assert_eq!(buffer.line_count(), 1); // Empty buffer has 1 empty line
    assert!(buffer.line(0).is_some());
    assert!(buffer.line(1).is_none());

    // Delete on empty should be safe
    buffer.delete(0, 0);
    buffer.delete(0, 100); // Over-delete

    // Undo on empty
    assert!(!buffer.undo());
    assert!(!buffer.redo());
}

#[test]
fn boundary_delete_operations() {
    let mut buffer = Buffer::from_string("abc");

    // Delete past end should be clamped
    buffer.delete(2, 100); // Delete from 'c' to way past end

    // Should still have "ab"
    assert_eq!(buffer.to_string(), "ab");
}

// =============================================================================
// SAVE INTEGRITY (requires temp files)
// =============================================================================

#[test]
fn atomic_save_creates_valid_file() {
    use std::io::Read;

    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_save.txt");

    // Create file first (save requires existing file for our test)
    std::fs::write(&file_path, "initial").unwrap();

    let mut buffer = Buffer::from_file(&file_path).unwrap();
    buffer.insert(0, "MODIFIED: ");
    buffer.save().unwrap();

    // Read back and verify
    let mut saved_content = String::new();
    std::fs::File::open(&file_path)
        .unwrap()
        .read_to_string(&mut saved_content)
        .unwrap();

    assert_eq!(saved_content, "MODIFIED: initial");
}

// =============================================================================
// NEW STRESS TESTS
// =============================================================================

#[test]
fn test_rapid_typing_simulation() {
    let mut buffer = Buffer::new();
    let typing_content =
        "The quick brown fox jumps over the lazy dog! 1234567890\n\t!@#$%^&*()_+ \n";
    let chars: Vec<char> = typing_content.chars().collect();
    let mut expected_len = 0;

    // Simulate typing 10,000 characters rapidly
    // With batch grouping (100 single-char edits per group), this creates ~100 groups
    // Well within MAX_UNDO_DEPTH of 10,000
    for i in 0..10_000 {
        let c = chars[i % chars.len()];
        let s = c.to_string();
        expected_len += s.len();
        buffer.insert(buffer.len(), &s);
    }

    assert_eq!(buffer.len(), expected_len);

    // Undo all and verify buffer empty
    while buffer.undo() {}
    assert_eq!(
        buffer.to_string(),
        "",
        "Buffer should be empty after undoing all rapid typing"
    );
}

#[test]
fn test_large_file_operations() {
    let mut buffer = Buffer::new();

    // Create buffer with 100,000 lines
    for i in 0..100_000 {
        buffer.insert(buffer.len(), &format!("Line {}\n", i));
    }

    // Verify line_count() works
    assert_eq!(buffer.line_count(), 100_001); // 100k lines + trailing empty line

    // Navigate to line 50,000
    let line_50k_offset = buffer
        .line_to_byte(50_000)
        .expect("Line 50,000 should exist");

    // Insert text there
    let insert_text = "STRESS TEST INSERTION\n";
    buffer.insert(line_50k_offset, insert_text);

    // Verify insertion
    assert_eq!(buffer.line_count(), 100_002);
    assert_eq!(
        buffer.line(50_000),
        Some("STRESS TEST INSERTION".to_string())
    );

    // Delete line 50,000
    let line_len = buffer.line_len(50_000).expect("Line 50,000 should exist");
    buffer.delete(line_50k_offset, line_len);

    // Verify deletion
    assert_eq!(buffer.line_count(), 100_001);
    assert_eq!(buffer.line(50_000), Some("Line 50000".to_string()));
}

#[test]
fn test_multi_window_stress() {
    let mut app = EditorApp::new();

    // Add 9 more windows, each on a different buffer (total 10)
    for i in 1..10 {
        app.split_window_vertically();
        let buffer = Buffer::from_string(&format!("Content of buffer {}", i));
        let bid = app.add_buffer(buffer);
        if let Some(win) = app.active_window_mut() {
            win.buffer_id = bid;
        }
    }

    assert_eq!(app.windows.len(), 10);
    assert_eq!(app.buffers.len(), 10);

    // Switch between windows rapidly
    let mut last_window = app.active_window;
    for _ in 0..100 {
        app.next_window();
        assert_ne!(
            app.active_window, last_window,
            "Focus should move to a different window"
        );

        // Verify state integrity
        let active_win_id = app.active_window;
        let window = app
            .windows
            .get(&active_win_id)
            .expect("Active window should exist");
        let buffer = app
            .buffers
            .get(&window.buffer_id)
            .expect("Buffer for active window should exist");

        // Minor check on buffer content
        if window.buffer_id.0 > 0 {
            assert!(buffer.to_string().starts_with("Content of buffer"));
        }

        last_window = active_win_id;
    }
}
