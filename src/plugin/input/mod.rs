//! Input handling submodules
//!
//! This module organizes input handlers by mode:
//! - builtin: Built-in fallback keymap (mirrors default_keymaps.gd)
//! - command: Command mode (:)
//! - dispatch: Key resolution state machine for all modes (normal/visual/insert/replace)
//! - pending: Pending operations (f/t/r, marks, macros, registers)
//! - search: Search mode (/, ?)

mod builtin;
mod command;
mod dispatch;
mod pending;
mod search;
