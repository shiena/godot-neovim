//! Input handling submodules
//!
//! This module organizes input handlers by mode:
//! - command: Command mode (:)
//! - search: Search mode (/, ?)
//! - insert: Insert mode
//! - replace: Replace mode
//! - pending: Pending operations (f/t/r, marks, macros, registers)
//! - normal: Normal mode (largest, may be further split)

mod command;
mod insert;
mod normal;
mod pending;
mod replace;
mod search;
