//! Godot-side insert-mode Escape sequences. Prefixes remain visible as text.

use super::GodotNeovimPlugin;
use godot::classes::{CodeEdit, InputEventKey};
use godot::prelude::*;
use std::time::{Duration, Instant};

pub(super) struct PendingEscape {
    editor: Gd<CodeEdit>,
    line: i32,
    column: i32,
    text: String,
    prefix: String,
    time: Instant,
}

/// Keep the longest suffix that can still begin an Escape binding.
fn resolve_sequence(previous: &str, key: char, bindings: &[String]) -> (bool, String) {
    let candidate = format!("{previous}{key}");
    for (offset, _) in candidate.char_indices() {
        let suffix = &candidate[offset..];
        let printable = || {
            bindings
                .iter()
                .filter(|binding| !(binding.starts_with('<') && binding.ends_with('>')))
        };
        if printable().any(|binding| binding == suffix) {
            return (true, suffix.to_owned());
        }
        if printable().any(|binding| binding.starts_with(suffix)) {
            return (false, suffix.to_owned());
        }
    }
    (false, String::new())
}

impl GodotNeovimPlugin {
    pub(super) fn resolve_insert_escape(&mut self, event: &Gd<InputEventKey>) -> bool {
        let pending = self.insert_escape_pending.take();
        if self.insert_escape_keys.is_empty() {
            return false;
        }
        let notation = self.key_event_to_nvim_string(event).unwrap_or_default();
        if notation.starts_with('<')
            && !self.is_alt_composed_unicode(event)
            && self.insert_escape_keys.contains(&notation)
        {
            self.record_insert_escape();
            return true;
        }
        if event.is_ctrl_pressed() || event.is_alt_pressed() || event.is_meta_pressed() {
            return false;
        }
        let Some(character) = char::from_u32(event.get_unicode()).filter(|c| !c.is_control())
        else {
            // Modifier-only presses do not interrupt a shifted sequence.
            if matches!(event.get_keycode(), godot::global::Key::SHIFT) {
                self.insert_escape_pending = pending;
            }
            return false;
        };
        let Some(mut editor) = self.current_editor.clone() else {
            return false;
        };
        if editor.has_selection() || editor.get_caret_count() != 1 {
            return false;
        }
        let line = editor.get_caret_line();
        let column = editor.get_caret_column();
        let text = editor.get_line(line).to_string();
        let previous = pending
            .as_ref()
            .filter(|p| {
                p.editor == editor
                    && p.line == line
                    && p.column == column
                    && p.text == text
                    && p.time.elapsed() < Duration::from_millis(1000)
            })
            .map(|p| p.prefix.as_str())
            .unwrap_or("");
        let (matched, prefix) = resolve_sequence(previous, character, &self.insert_escape_keys);
        if matched {
            let count = prefix.chars().count() - 1;
            if count > 0 {
                // The final character is consumed; remove only the verified prefix.
                editor.remove_text(line, column - count as i32, line, column);
                editor.set_caret_column(column - count as i32);
                if self.recording_macro.is_some() && !self.playing_macro {
                    self.macro_buffer
                        .truncate(self.macro_buffer.len().saturating_sub(count));
                }
            }
            self.record_insert_escape();
            return true;
        }
        if !prefix.is_empty() {
            let mut expected: Vec<char> = text.chars().collect();
            expected.insert(column as usize, character);
            self.insert_escape_pending = Some(PendingEscape {
                editor,
                line,
                column: column + 1,
                text: expected.into_iter().collect(),
                prefix,
                time: Instant::now(),
            });
        }
        false
    }

    fn record_insert_escape(&mut self) {
        if self.recording_macro.is_some() && !self.playing_macro {
            self.macro_buffer.push("<Esc>".to_owned());
        }
        self.send_escape();
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_sequence;

    #[test]
    fn escape_and_mismatch() {
        let bindings = vec!["kj".to_owned()];
        assert_eq!(resolve_sequence("", 'k', &bindings), (false, "k".into()));
        assert_eq!(resolve_sequence("k", 'j', &bindings), (true, "kj".into()));
        assert_eq!(resolve_sequence("k", 'x', &bindings), (false, "".into()));
        assert_eq!(resolve_sequence("k", 'k', &bindings), (false, "k".into()));
    }

    #[test]
    fn unicode_and_longer_bindings() {
        let bindings = vec!["あい".to_owned(), "jjk".to_owned()];
        assert_eq!(
            resolve_sequence("あ", 'い', &bindings),
            (true, "あい".into())
        );
        assert_eq!(resolve_sequence("jj", 'j', &bindings), (false, "jj".into()));
        assert_eq!(resolve_sequence("jj", 'k', &bindings), (true, "jjk".into()));
    }

    #[test]
    fn special_key_notation_is_not_a_printable_sequence() {
        assert_eq!(
            resolve_sequence("", '<', &["<C-k>".into()]),
            (false, "".into())
        );
    }
}
