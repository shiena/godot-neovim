//! Command-line mode and Ex commands
//!
//! This module organizes command handlers by category:
//! - mode: Command-line mode management (open/close, history)
//! - file_ops: File operations (:w, :q, :e, etc.)
//! - text_ops: Text manipulation (:s, :g, :sort, :t, :m)
//! - buffer_nav: Buffer/tab navigation (:bn, :bp, gt, gT)
//! - info: Information display (:marks, :registers, :jumps, :ls)
//! - help: Help and documentation (:help, :version, K)

mod buffer_nav;
mod file_ops;
mod help;
mod info;
mod mode;
mod text_ops;
