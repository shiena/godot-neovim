//! Compatibility helpers for legacy editors and Godot 4.8 document containers.

use godot::classes::{CodeEdit, EditorInterface, ItemList, MenuButton, Node, PopupMenu, Shortcut};
use godot::prelude::*;

/// Match the registered Shortcut resource, independent of translation, key bindings, and IDs.
pub(super) fn menu_item_id(popup: &Gd<PopupMenu>, shortcut_path: &str) -> Option<i32> {
    let mut settings = EditorInterface::singleton().get_editor_settings()?;
    if !settings.has_method("get_shortcut") {
        // Older engines do not bind EditorSettings.get_shortcut. Resource names
        // are the untranslated TTRC strings, even when menu labels are translated.
        let name = match shortcut_path {
            "script_editor/save" => "Save",
            "script_editor/save_all" => "Save All",
            "script_editor/close_file" => "Close",
            "script_editor/close_all" => "Close All",
            "shader_editor/open" | "script_editor/open_shader" => "Load Shader File...",
            _ => return None,
        };
        return (0..popup.get_item_count()).find_map(|index| {
            popup.get_item_shortcut(index).and_then(|shortcut| {
                (shortcut.get_name() == name).then(|| popup.get_item_id(index))
            })
        });
    }
    let shortcut = settings
        .call("get_shortcut", &[shortcut_path.to_variant()])
        .try_to::<Gd<Shortcut>>()
        .ok()?;
    (0..popup.get_item_count()).find_map(|index| {
        (popup.get_item_shortcut(index).as_ref() == Some(&shortcut))
            .then(|| popup.get_item_id(index))
    })
}

/// Search menu buttons only, excluding transient context menus.
pub(super) fn find_menu(node: &Gd<Node>, shortcut_path: &str) -> Option<Gd<PopupMenu>> {
    if let Ok(button) = node.clone().try_cast::<MenuButton>() {
        let popup = button.get_popup()?;
        if menu_item_id(&popup, shortcut_path).is_some() {
            return Some(popup);
        }
    }
    for child in node.get_children().iter_shared() {
        if let Some(popup) = find_menu(&child, shortcut_path) {
            return Some(popup);
        }
    }
    None
}

pub(super) fn emit_menu_action(mut popup: Gd<PopupMenu>, shortcut_path: &str) -> bool {
    let Some(id) = menu_item_id(&popup, shortcut_path) else {
        return false;
    };
    popup.call_deferred("emit_signal", &["id_pressed".to_variant(), id.to_variant()]);
    true
}

/// The file list is direct in legacy split containers and nested in 4.8 DocumentList.
pub(super) fn document_list(node: &Gd<Node>) -> Option<Gd<ItemList>> {
    for child in node.get_children().iter_shared() {
        if let Ok(list) = child.clone().try_cast::<ItemList>() {
            return Some(list);
        }
        // Do not enter editor tabs or the outline, which may contain unrelated lists.
        let class = child.get_class().to_string();
        if matches!(
            class.as_str(),
            "VBoxContainer" | "HSplitContainer" | "VSplitContainer" | "DocumentList"
        ) {
            if let Some(list) = document_list(&child) {
                return Some(list);
            }
        }
    }
    None
}

/// Identify the shader container by its shader-only File menu action.
pub(super) fn find_shader_container(node: &Gd<Node>) -> Option<Gd<Node>> {
    let class = node.get_class().to_string();
    let shortcut = match class.as_str() {
        // Early 4.8 builds used a second ScriptEditor for shaders.
        "DocumentEditorContainer" | "ScriptEditor" => Some("script_editor/open_shader"),
        "HSplitContainer" => Some("shader_editor/open"),
        _ => None,
    };
    if let Some(shortcut) = shortcut {
        if document_list(node).is_some() && find_menu(node, shortcut).is_some() {
            return Some(node.clone());
        }
    }
    for child in node.get_children().iter_shared() {
        if let Some(container) = find_shader_container(&child) {
            return Some(container);
        }
    }
    None
}

pub(super) fn visible_code_edit(node: &Gd<Node>) -> Option<Gd<CodeEdit>> {
    if let Ok(editor) = node.clone().try_cast::<CodeEdit>() {
        return editor.is_visible_in_tree().then_some(editor);
    }
    for child in node.get_children().iter_shared() {
        if let Some(editor) = visible_code_edit(&child) {
            return Some(editor);
        }
    }
    None
}
