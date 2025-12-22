//! PTY-based terminal emulator host using VTE parser.
//!
//! This module provides a robust VT100/xterm compliant terminal emulator
//! backed by the VTE parser for ANSI escape sequence handling.

#![allow(dead_code)]

use portable_pty::{Child, CommandBuilder, NativePtySystem, PtyPair, PtySize, PtySystem};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(feature = "terminal")]
use vte::{Params, Parser, Perform};

/// VTE handler that updates our grid
#[cfg(feature = "terminal")]
struct GridHandler {
    grid: Vec<Vec<char>>,
    cols: usize,
    rows: usize,
    cursor_x: usize,
    cursor_y: usize,
}

#[cfg(feature = "terminal")]
impl GridHandler {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: vec![vec![' '; cols]; rows],
            cols,
            rows,
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.grid.resize(rows, vec![' '; cols]);
        for row in self.grid.iter_mut() {
            row.resize(cols, ' ');
        }
        if self.cursor_y >= rows {
            self.cursor_y = rows.saturating_sub(1);
        }
        if self.cursor_x >= cols {
            self.cursor_x = cols.saturating_sub(1);
        }
    }

    fn put_char(&mut self, c: char) {
        if self.cursor_y < self.rows && self.cursor_x < self.cols {
            self.grid[self.cursor_y][self.cursor_x] = c;
            self.cursor_x += 1;
            if self.cursor_x >= self.cols {
                self.cursor_x = 0;
                self.line_feed();
            }
        }
    }

    fn line_feed(&mut self) {
        self.cursor_y += 1;
        if self.cursor_y >= self.rows {
            // Scroll up
            self.grid.remove(0);
            self.grid.push(vec![' '; self.cols]);
            self.cursor_y = self.rows - 1;
        }
    }

    fn clear_line_from_cursor(&mut self) {
        if self.cursor_y < self.rows {
            for x in self.cursor_x..self.cols {
                self.grid[self.cursor_y][x] = ' ';
            }
        }
    }

    fn clear_screen(&mut self) {
        for row in &mut self.grid {
            for c in row.iter_mut() {
                *c = ' ';
            }
        }
    }
}

#[cfg(feature = "terminal")]
impl Perform for GridHandler {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => {
                // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            0x09 => {
                // Tab
                let next_tab = ((self.cursor_x / 8) + 1) * 8;
                self.cursor_x = next_tab.min(self.cols.saturating_sub(1));
            }
            0x0A | 0x0B | 0x0C => {
                // LF, VT, FF
                self.line_feed();
            }
            0x0D => {
                // CR
                self.cursor_x = 0;
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let params: Vec<u16> = params
            .iter()
            .map(|p| match p.first().copied() {
                Some(v) => v,
                None => 0,
            })
            .collect();

        match action {
            'A' => {
                // Cursor Up
                let n = match params.first().copied() {
                    Some(v) => v.max(1) as usize,
                    None => 1,
                };
                self.cursor_y = self.cursor_y.saturating_sub(n);
            }
            'B' => {
                // Cursor Down
                let n = match params.first().copied() {
                    Some(v) => v.max(1) as usize,
                    None => 1,
                };
                self.cursor_y = (self.cursor_y + n).min(self.rows.saturating_sub(1));
            }
            'C' => {
                // Cursor Forward
                let n = match params.first().copied() {
                    Some(v) => v.max(1) as usize,
                    None => 1,
                };
                self.cursor_x = (self.cursor_x + n).min(self.cols.saturating_sub(1));
            }
            'D' => {
                // Cursor Back
                let n = match params.first().copied() {
                    Some(v) => v.max(1) as usize,
                    None => 1,
                };
                self.cursor_x = self.cursor_x.saturating_sub(n);
            }
            'H' | 'f' => {
                // Cursor Position
                let row = match params.first().copied() {
                    Some(v) => v.max(1) as usize,
                    None => 1,
                };
                let col = match params.get(1).copied() {
                    Some(v) => v.max(1) as usize,
                    None => 1,
                };
                self.cursor_y = (row - 1).min(self.rows.saturating_sub(1));
                self.cursor_x = (col - 1).min(self.cols.saturating_sub(1));
            }
            'J' => {
                // Erase in Display
                let mode = match params.first().copied() {
                    Some(v) => v,
                    None => 0,
                };
                match mode {
                    0 => {
                        // Clear from cursor to end
                        self.clear_line_from_cursor();
                        for y in (self.cursor_y + 1)..self.rows {
                            for c in self.grid[y].iter_mut() {
                                *c = ' ';
                            }
                        }
                    }
                    1 => {
                        // Clear from start to cursor
                        for y in 0..self.cursor_y {
                            for c in self.grid[y].iter_mut() {
                                *c = ' ';
                            }
                        }
                        for x in 0..=self.cursor_x.min(self.cols.saturating_sub(1)) {
                            if self.cursor_y < self.rows {
                                self.grid[self.cursor_y][x] = ' ';
                            }
                        }
                    }
                    2 | 3 => {
                        // Clear entire screen
                        self.clear_screen();
                    }
                    _ => {}
                }
            }
            'K' => {
                // Erase in Line
                let mode = match params.first().copied() {
                    Some(v) => v,
                    None => 0,
                };
                if self.cursor_y < self.rows {
                    match mode {
                        0 => {
                            // Clear from cursor to end
                            for x in self.cursor_x..self.cols {
                                self.grid[self.cursor_y][x] = ' ';
                            }
                        }
                        1 => {
                            // Clear from start to cursor
                            for x in 0..=self.cursor_x.min(self.cols.saturating_sub(1)) {
                                self.grid[self.cursor_y][x] = ' ';
                            }
                        }
                        2 => {
                            // Clear entire line
                            for c in self.grid[self.cursor_y].iter_mut() {
                                *c = ' ';
                            }
                        }
                        _ => {}
                    }
                }
            }
            'm' => { /* SGR - ignore colors for now, just handle text */ }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'c' => {
                // Reset
                self.clear_screen();
                self.cursor_x = 0;
                self.cursor_y = 0;
            }
            b'D' => {
                // Line feed
                self.line_feed();
            }
            b'M' => {
                // Reverse line feed
                if self.cursor_y > 0 {
                    self.cursor_y -= 1;
                }
            }
            _ => {}
        }
    }
}

/// TerminalHost manages a PTY-based terminal session using VTE parser.
pub struct TerminalHost {
    /// PTY pair (master and slave sides)
    pub pty: PtyPair,
    /// Handle to the child process running in the PTY
    pub child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    /// Reader for PTY output
    pub reader: Box<dyn Read + Send>,
    /// Writer for PTY input  
    writer: Box<dyn Write + Send>,

    #[cfg(feature = "terminal")]
    /// VTE parser
    parser: Parser,

    #[cfg(feature = "terminal")]
    /// Grid handler
    handler: GridHandler,

    #[cfg(not(feature = "terminal"))]
    /// Fallback grid for when terminal feature is disabled
    pub grid: Vec<Vec<char>>,

    /// Current number of columns
    pub cols: u16,
    /// Current number of rows
    pub rows: u16,
    /// Internal receiver for non-blocking reads from the PTY
    rx: Receiver<Vec<u8>>,
}

impl TerminalHost {
    /// Spawns a new terminal session with the specified shell and dimensions.
    pub fn spawn(shell: &str, cols: u16, rows: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = NativePtySystem::default();
        let pty_pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let cmd = CommandBuilder::new(shell);
        let child = pty_pair.slave.spawn_command(cmd)?;

        let reader = pty_pair.master.try_clone_reader()?;
        let writer = pty_pair.master.take_writer()?;

        let (tx, rx) = mpsc::channel();
        let mut t_reader = pty_pair.master.try_clone_reader()?;

        // Spawn a background thread to read from the PTY and send to the channel
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = t_reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        #[cfg(feature = "terminal")]
        {
            Ok(Self {
                pty: pty_pair,
                child: Arc::new(Mutex::new(child)),
                reader,
                writer,
                parser: Parser::new(),
                handler: GridHandler::new(cols as usize, rows as usize),
                cols,
                rows,
                rx,
            })
        }

        #[cfg(not(feature = "terminal"))]
        {
            let grid = vec![vec![' '; cols as usize]; rows as usize];
            Ok(Self {
                pty: pty_pair,
                child: Arc::new(Mutex::new(child)),
                reader,
                writer,
                grid,
                cols,
                rows,
                rx,
            })
        }
    }

    /// Resizes the PTY and the internal terminal.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let _ = self.pty.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });

        self.cols = cols;
        self.rows = rows;

        #[cfg(feature = "terminal")]
        {
            self.handler.resize(cols as usize, rows as usize);
        }

        #[cfg(not(feature = "terminal"))]
        {
            self.grid.resize(rows as usize, vec![' '; cols as usize]);
            for row in self.grid.iter_mut() {
                row.resize(cols as usize, ' ');
            }
        }
    }

    /// Writes data to the PTY input.
    pub fn write(&mut self, data: &[u8]) {
        let _ = self.writer.write_all(data);
        let _ = self.writer.flush();
    }

    /// Non-blocking read from the PTY. Returns new data as a String if available.
    /// Also updates the internal terminal emulator state via VTE parser.
    pub fn read(&mut self) -> Option<String> {
        let mut accumulated = String::new();
        let mut found = false;

        while let Ok(data) = self.rx.try_recv() {
            if let Ok(s) = String::from_utf8(data.clone()) {
                accumulated.push_str(&s);
                found = true;
            }

            #[cfg(feature = "terminal")]
            {
                // Feed bytes to VTE parser
                self.parser.advance(&mut self.handler, &data);
            }
        }

        if found { Some(accumulated) } else { None }
    }

    /// Checks if the child process is still alive.
    pub fn is_alive(&self) -> bool {
        if let Ok(mut child) = self.child.lock() {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Get entire grid as Vec<Vec<char>> for rendering
    pub fn grid(&self) -> Vec<Vec<char>> {
        #[cfg(feature = "terminal")]
        {
            self.handler.grid.clone()
        }

        #[cfg(not(feature = "terminal"))]
        {
            self.grid.clone()
        }
    }
}

impl Drop for TerminalHost {
    fn drop(&mut self) {
        // Send SIGHUP to child process
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}
