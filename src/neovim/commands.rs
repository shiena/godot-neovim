//! Command types for Neovim communication
//!
//! Inspired by neovide's architecture, commands are categorized as:
//! - Serial: Must be processed in order (keyboard input) - uses non-blocking send
//! - Parallel: Can return immediately or be processed concurrently (queries)

/// Commands that must be processed in order (keyboard input)
/// These are sent without waiting for response to maintain responsiveness
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum SerialCommand {
    /// Send keyboard input to Neovim (feedkeys)
    Input(String),
    /// Set cursor position (line: 1-indexed, col: 0-indexed)
    SetCursor { line: i64, col: i64 },
}

/// Commands that query or modify state
/// These may block waiting for response when the result is needed
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ParallelCommand {
    /// Set buffer content
    SetBufferLines {
        start: i64,
        end: i64,
        lines: Vec<String>,
    },
    /// Get buffer content
    GetBufferLines { start: i64, end: i64 },
    /// Get line count
    GetLineCount,
    /// Set buffer name
    SetBufferName(String),
    /// Set buffer as not modified
    SetBufferNotModified,
    /// Get visual selection range
    GetVisualSelection,
    /// Get current mode
    GetMode,
    /// Get cursor position
    GetCursor,
}
