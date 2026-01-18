//! Buffer synchronization manager (ComradeNeovim-style)
//!
//! Implements changedtick-based synchronization with Neovim as master.

use std::collections::HashMap;

/// Buffer change event from Neovim
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BufLinesEvent {
    /// Buffer ID
    pub buf: i64,
    /// Change tick (monotonically increasing)
    pub changedtick: i64,
    /// First line changed (0-indexed)
    pub first_line: i64,
    /// Last line changed (exclusive, -1 means to end)
    pub last_line: i64,
    /// New line data (empty for deletion)
    pub line_data: Vec<String>,
    /// More changes coming
    pub more: bool,
}

/// Buffer change to apply to Godot editor
#[derive(Debug, Clone)]
pub struct DocumentChange {
    /// First line to replace (0-indexed)
    pub first_line: i64,
    /// Last line to replace (exclusive)
    pub last_line: i64,
    /// New lines to insert
    pub new_lines: Vec<String>,
}

/// Manages buffer synchronization between Neovim and Godot
pub struct SyncManager {
    /// Neovim's buffer change counter
    changedtick: i64,

    /// Flag: currently processing a change from Neovim
    /// Used to prevent echo (Godot change -> Neovim -> back to Godot)
    changed_by_nvim: bool,

    /// Pending changes from Godot (tick -> change)
    /// When Neovim confirms with changedtick, we check here to ignore echoes
    pending_changes: HashMap<i64, DocumentChange>,

    /// Buffer attached flag
    attached: bool,

    /// Initial sync tick - events with this tick are echoes of initial sync
    initial_sync_tick: Option<i64>,

    /// Neovim buffer line count (used to clamp cursor position)
    nvim_line_count: i32,
}

impl SyncManager {
    pub fn new() -> Self {
        Self {
            changedtick: -1,
            changed_by_nvim: false,
            pending_changes: HashMap::new(),
            attached: false,
            initial_sync_tick: None,
            nvim_line_count: 0,
        }
    }

    /// Reset state (for new buffer)
    pub fn reset(&mut self) {
        self.changedtick = -1;
        self.changed_by_nvim = false;
        self.pending_changes.clear();
        self.attached = false;
        self.initial_sync_tick = None;
        self.nvim_line_count = 0;
    }

    /// Set Neovim buffer line count
    pub fn set_line_count(&mut self, count: i32) {
        self.nvim_line_count = count;
    }

    /// Get Neovim buffer line count
    pub fn get_line_count(&self) -> i32 {
        self.nvim_line_count
    }

    /// Set initial sync tick to ignore echoes from initial buffer sync
    pub fn set_initial_sync_tick(&mut self, tick: i64) {
        self.initial_sync_tick = Some(tick);
        self.changedtick = tick;
        crate::verbose_print!("[SyncManager] Initial sync tick set to {}", tick);
    }

    /// Mark buffer as attached
    pub fn set_attached(&mut self, attached: bool) {
        self.attached = attached;
        if !attached {
            self.reset();
        }
    }

    /// Process buffer lines event from Neovim
    /// Returns Some(change) if Godot should update, None if echo
    pub fn on_nvim_buf_lines(&mut self, event: BufLinesEvent) -> Option<DocumentChange> {
        // Check if this is an echo of initial sync
        if let Some(initial_tick) = self.initial_sync_tick {
            if event.changedtick <= initial_tick {
                crate::verbose_print!(
                    "[SyncManager] Ignoring initial sync echo for tick {} (initial={})",
                    event.changedtick,
                    initial_tick
                );
                // Clear initial sync tick after first echo is ignored
                self.initial_sync_tick = None;
                return None;
            }
            // Clear initial sync tick if we received a newer tick
            self.initial_sync_tick = None;
        }

        // Check if this is an echo of our own change
        if self.is_echo(event.changedtick) {
            crate::verbose_print!("[SyncManager] Ignoring echo for tick {}", event.changedtick);
            return None;
        }

        // Validate changedtick order and reject duplicates
        if self.changedtick != -1 {
            if event.changedtick <= self.changedtick {
                // Same or older tick - this is a duplicate event, ignore it
                crate::verbose_print!(
                    "[SyncManager] Ignoring duplicate/old tick: current={}, got={}",
                    self.changedtick,
                    event.changedtick
                );
                return None;
            } else if event.changedtick != self.changedtick + 1 {
                // Out of order (skipped ticks) - accept but log warning
                crate::verbose_print!(
                    "[SyncManager] Out of order tick: expected {}, got {}",
                    self.changedtick + 1,
                    event.changedtick
                );
            }
        }

        self.changedtick = event.changedtick;

        // Update line count based on the change
        // delta = new_lines.len() - (last_line - first_line)
        let old_lines = (event.last_line - event.first_line) as i32;
        let new_lines = event.line_data.len() as i32;
        self.nvim_line_count += new_lines - old_lines;

        // Return change for Godot to apply
        Some(DocumentChange {
            first_line: event.first_line,
            last_line: event.last_line,
            new_lines: event.line_data,
        })
    }

    /// Process changedtick event (no content change)
    pub fn on_nvim_changedtick(&mut self, tick: i64) {
        if self.is_echo(tick) {
            crate::verbose_print!("[SyncManager] Ignoring changedtick echo for {}", tick);
            return;
        }
        self.changedtick = tick;
    }

    /// Check if change is an echo of our pending change
    fn is_echo(&mut self, tick: i64) -> bool {
        self.pending_changes.remove(&tick).is_some()
    }

    /// Set flag when applying Neovim change to Godot
    pub fn begin_nvim_change(&mut self) {
        self.changed_by_nvim = true;
    }

    /// Clear flag after applying Neovim change
    pub fn end_nvim_change(&mut self) {
        self.changed_by_nvim = false;
    }
}

impl Default for SyncManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nvim_change() {
        let mut sync = SyncManager::new();

        // Receive change from Neovim (first change, tick 1)
        let event = BufLinesEvent {
            buf: 1,
            changedtick: 1,
            first_line: 0,
            last_line: 1,
            line_data: vec!["new line".to_string()],
            more: false,
        };

        // Should return change to apply
        let change = sync.on_nvim_buf_lines(event);
        assert!(change.is_some());
        let change = change.unwrap();
        assert_eq!(change.first_line, 0);
        assert_eq!(change.last_line, 1);
        assert_eq!(change.new_lines, vec!["new line".to_string()]);
    }

    // Note: Tests for duplicate tick detection and initial sync echo
    // are not included here because they hit verbose_print! paths
    // which require Godot engine. These are tested manually.
}
