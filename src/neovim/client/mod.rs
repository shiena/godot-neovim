//! Neovim client module
//!
//! This module organizes the NeovimClient into submodules:
//! - connection: Process management (new, start, stop)
//! - state: State polling (take_state, take_viewport, poll)
//! - input: Key input (input, send_keys, channels)
//! - buffer: Buffer operations (buffer_update, switch_to_buffer, attach)
//! - cursor: Cursor and visual selection
//! - execution: Command and Lua execution

mod buffer;
mod connection;
mod cursor;
mod execution;
mod input;
mod state;

use crate::neovim::{NeovimHandler, NeovimState};
use nvim_rs::Neovim;
use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
pub(super) const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Minimum required Neovim version (major, minor, patch)
pub(super) const NEOVIM_REQUIRED_VERSION: (u64, u64, u64) = (0, 9, 0);

/// Default timeout for RPC commands (milliseconds)
pub(super) const RPC_TIMEOUT_MS: u64 = 100;

/// Extended timeout for operations that may trigger dialogs (e.g., swap file)
pub(super) const RPC_EXTENDED_TIMEOUT_MS: u64 = 500;

/// Fallback Lua code when external plugin is not available
/// Prefer using the external lua/godot_neovim/init.lua file
pub(super) const LUA_FALLBACK_CODE: &str = r#"
-- godot_neovim: Buffer management for Godot integration (fallback)
_G.godot_neovim = {}

-- Register buffer with initial content (clears undo history)
function _G.godot_neovim.buffer_register(bufnr, lines)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end
    local saved_ul = vim.bo[bufnr].undolevels
    vim.bo[bufnr].undolevels = -1
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)
    vim.bo[bufnr].undolevels = saved_ul
    vim.bo[bufnr].modified = false
    return vim.api.nvim_buf_get_changedtick(bufnr)
end

-- Update buffer content (preserves undo history for 'u' command)
function _G.godot_neovim.buffer_update(bufnr, lines)
    if bufnr == 0 then
        bufnr = vim.api.nvim_get_current_buf()
    end
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, lines)
    return vim.api.nvim_buf_get_changedtick(bufnr)
end
"#;

pub(super) type Writer = nvim_rs::compat::tokio::Compat<tokio::process::ChildStdin>;

/// Neovim version information
#[derive(Debug, Clone, Default)]
pub struct NeovimVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl NeovimVersion {
    /// Check if version meets minimum requirements
    pub fn meets_requirement(&self, major: u64, minor: u64, patch: u64) -> bool {
        if self.major > major {
            return true;
        }
        if self.major < major {
            return false;
        }
        // major is equal
        if self.minor > minor {
            return true;
        }
        if self.minor < minor {
            return false;
        }
        // minor is equal
        self.patch >= patch
    }
}

impl fmt::Display for NeovimVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Result from switching to a buffer
#[derive(Debug, Clone)]
pub struct SwitchBufferResult {
    /// Buffer number in Neovim
    pub bufnr: i64,
    /// Current changedtick
    pub tick: i64,
    /// Whether this is a newly created buffer
    pub is_new: bool,
    /// Whether buffer is attached for notifications
    pub attached: bool,
    /// Cursor position (line, col) - line is 1-indexed, col is 0-indexed
    pub cursor: (i64, i64),
}

/// Indent options for Neovim buffer
#[derive(Debug, Clone, Copy)]
pub struct IndentOptions {
    /// Use spaces instead of tabs
    pub use_spaces: bool,
    /// Indent size (number of spaces or tab width)
    pub indent_size: i32,
}

/// Manages connection to Neovim process
pub struct NeovimClient {
    pub(super) runtime: Runtime,
    pub(super) neovim: Arc<Mutex<Option<Neovim<Writer>>>>,
    pub(super) handler: NeovimHandler,
    pub(super) nvim_path: String,
    /// Start Neovim with --clean flag (no plugins or user config)
    pub(super) clean: bool,
    /// Shared state from handler (mode, cursor position)
    pub(super) state: Arc<Mutex<NeovimState>>,
    /// Flag indicating new updates from redraw events
    pub(super) has_updates: Arc<AtomicBool>,
    /// IO handler task - must be kept alive for events to be received
    #[allow(dead_code)]
    pub(super) io_handle:
        Option<tokio::task::JoinHandle<Result<(), Box<nvim_rs::error::LoopError>>>>,
    /// Key input channel sender (unbounded for no key drops)
    pub(super) key_input_tx: Option<UnboundedSender<String>>,
    /// Key input processor task handle
    #[allow(dead_code)]
    pub(super) key_input_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Default for NeovimClient {
    fn default() -> Self {
        Self::new().expect("Failed to create NeovimClient")
    }
}

impl Drop for NeovimClient {
    fn drop(&mut self) {
        self.stop();
        // Force non-blocking runtime shutdown to avoid hanging
        // Note: This consumes self.runtime, but we're in drop so that's fine
        crate::verbose_print!("[godot-neovim] Shutting down runtime");
    }
}
