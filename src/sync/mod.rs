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
}

impl SyncManager {
    pub fn new() -> Self {
        Self {
            changedtick: -1,
            changed_by_nvim: false,
            pending_changes: HashMap::new(),
            attached: false,
            initial_sync_tick: None,
        }
    }

    /// Reset state (for new buffer)
    pub fn reset(&mut self) {
        self.changedtick = -1;
        self.changed_by_nvim = false;
        self.pending_changes.clear();
        self.attached = false;
        self.initial_sync_tick = None;
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

    /// Check if buffer is attached
    #[allow(dead_code)]
    pub fn is_attached(&self) -> bool {
        self.attached
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

        // Validate changedtick order
        if self.changedtick != -1 && event.changedtick != self.changedtick + 1 {
            // Out of order - might need resync
            crate::verbose_print!(
                "[SyncManager] Out of order tick: expected {}, got {}",
                self.changedtick + 1,
                event.changedtick
            );
            // For now, accept it anyway
        }

        self.changedtick = event.changedtick;

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

    /// Register a pending change from Godot (e.g., Undo/Redo)
    /// Returns the expected tick number
    #[allow(dead_code)]
    pub fn register_godot_change(&mut self, change: DocumentChange) -> i64 {
        let expected_tick = self.changedtick + 1;
        self.pending_changes.insert(expected_tick, change);
        expected_tick
    }

    /// Set flag when applying Neovim change to Godot
    pub fn begin_nvim_change(&mut self) {
        self.changed_by_nvim = true;
    }

    /// Clear flag after applying Neovim change
    pub fn end_nvim_change(&mut self) {
        self.changed_by_nvim = false;
    }

    /// Check if currently applying Neovim change
    #[allow(dead_code)]
    pub fn is_nvim_change(&self) -> bool {
        self.changed_by_nvim
    }

    /// Get current changedtick
    #[allow(dead_code)]
    pub fn changedtick(&self) -> i64 {
        self.changedtick
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
    fn test_echo_detection() {
        let mut sync = SyncManager::new();
        sync.changedtick = 5;

        // Register a Godot change
        let change = DocumentChange {
            first_line: 0,
            last_line: 1,
            new_lines: vec!["test".to_string()],
        };
        let tick = sync.register_godot_change(change);
        assert_eq!(tick, 6);

        // Receive echo from Neovim
        let event = BufLinesEvent {
            buf: 1,
            changedtick: 6,
            first_line: 0,
            last_line: 1,
            line_data: vec!["test".to_string()],
            more: false,
        };

        // Should be detected as echo
        assert!(sync.on_nvim_buf_lines(event).is_none());
    }

    #[test]
    fn test_nvim_change() {
        let mut sync = SyncManager::new();
        sync.changedtick = 5;

        // Receive change from Neovim
        let event = BufLinesEvent {
            buf: 1,
            changedtick: 6,
            first_line: 0,
            last_line: 1,
            line_data: vec!["new line".to_string()],
            more: false,
        };

        // Should return change to apply
        let change = sync.on_nvim_buf_lines(event);
        assert!(change.is_some());
        assert_eq!(sync.changedtick(), 6);
    }
}
