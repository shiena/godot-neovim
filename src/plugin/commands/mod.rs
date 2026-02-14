//! Command-line mode and Ex commands
//!
//! This module organizes command handlers by category:
//! - mode: Command-line mode management (open/close, history)
//! - file_ops: File operations (:w, :q, :e, etc.)
//! - buffer_nav: Buffer/tab navigation (:bn, :bp, gt, gT)
//! - info: Information display (:marks, :registers, :jumps, :ls)
//! - help: Help and documentation (:help, :version, K)

use godot::classes::{Input, InputEventKey};
use godot::global::Key;
use godot::prelude::*;

mod buffer_nav;
mod file_ops;
mod help;
mod info;
mod mode;

/// Simulate a key press and release with optional modifiers
/// This triggers Godot's internal shortcut handling
pub(super) fn simulate_key_press(key: Key, ctrl: bool, shift: bool, alt: bool) {
    let mut key_press = InputEventKey::new_gd();
    key_press.set_keycode(key);
    key_press.set_ctrl_pressed(ctrl);
    key_press.set_shift_pressed(shift);
    key_press.set_alt_pressed(alt);
    key_press.set_pressed(true);
    Input::singleton().parse_input_event(&key_press);

    let mut key_release = InputEventKey::new_gd();
    key_release.set_keycode(key);
    key_release.set_ctrl_pressed(ctrl);
    key_release.set_shift_pressed(shift);
    key_release.set_alt_pressed(alt);
    key_release.set_pressed(false);
    Input::singleton().parse_input_event(&key_release);
}

/// Simulate Ctrl+S to trigger Godot's save with all EditorPlugin hooks
#[allow(dead_code)]
pub(super) fn simulate_ctrl_s() {
    simulate_key_press(Key::S, true, false, false);
}

/// Simulate Ctrl+W to close the current tab
pub(super) fn simulate_ctrl_w() {
    simulate_key_press(Key::W, true, false, false);
}

/// Simulate Ctrl+F to open the search dialog
pub(super) fn simulate_ctrl_f() {
    simulate_key_press(Key::F, true, false, false);
}
