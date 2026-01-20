//! Help and documentation: :help, :version, K

use super::super::{GodotNeovimPlugin, HelpMemberType, HelpQuery};
use godot::classes::ProjectSettings;
use godot::prelude::*;

impl GodotNeovimPlugin {
    /// :help - Open GodotNeovim help
    pub(in crate::plugin) fn cmd_help(&mut self) {
        self.pending_help_query = Some(HelpQuery {
            class_name: "GodotNeovim".to_string(),
            member_name: None,
            member_type: HelpMemberType::Class,
        });
    }

    /// :version - Show godot-neovim version in status label
    pub(in crate::plugin) fn cmd_version(&mut self) {
        self.show_version = true;
        self.update_version_display();
    }

    /// K - Open documentation for word under cursor
    /// Uses LSP hover to get class/member information for methods, properties, and signals
    /// Note: Actual goto_help() call is deferred to process() to avoid borrow conflicts
    /// (goto_help triggers editor_script_changed signal synchronously)
    pub(in crate::plugin) fn open_documentation(&mut self) {
        let Some(ref editor) = self.current_editor else {
            return;
        };

        // Get word under cursor
        let line_idx = editor.get_caret_line();
        let col_idx = editor.get_caret_column() as usize;
        let line_text = editor.get_line(line_idx).to_string();
        let chars: Vec<char> = line_text.chars().collect();

        if col_idx >= chars.len() {
            return;
        }

        // Find word boundaries
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
        let mut start = col_idx;
        while start > 0 && is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = col_idx;
        while end < chars.len() && is_word_char(chars[end]) {
            end += 1;
        }

        if start == end {
            return;
        }

        let word: String = chars[start..end].iter().collect();
        crate::verbose_print!("[godot-neovim] K: Looking up help for '{}'", word);

        // If word starts with uppercase, assume it's a class name (fast path)
        if word.chars().next().is_some_and(|c| c.is_uppercase()) {
            self.pending_help_query = Some(HelpQuery {
                class_name: word.clone(),
                member_name: None,
                member_type: HelpMemberType::Class,
            });
            crate::verbose_print!("[godot-neovim] K: Queueing class help for '{}'", word);
            return;
        }

        // Try LSP hover to get class/member information
        let Some(ref lsp) = self.godot_lsp else {
            crate::verbose_print!("[godot-neovim] K: LSP not available, skipping '{}'", word);
            return;
        };

        // Get absolute file path and convert to URI
        let abs_path = if self.current_script_path.starts_with("res://") {
            ProjectSettings::singleton()
                .globalize_path(&self.current_script_path)
                .to_string()
        } else {
            self.current_script_path.clone()
        };

        let uri = if abs_path.starts_with('/') {
            format!("file://{}", abs_path)
        } else {
            format!("file:///{}", abs_path.replace('\\', "/"))
        };

        // Get project root for LSP initialization
        let project_root = ProjectSettings::singleton()
            .globalize_path("res://")
            .to_string();
        let root_uri = if project_root.starts_with('/') {
            format!("file://{}", project_root)
        } else {
            format!("file:///{}", project_root.replace('\\', "/"))
        };

        // Ensure LSP is connected and initialized
        if !lsp.is_connected() {
            if let Err(e) = lsp.connect(6005) {
                crate::verbose_print!("[godot-neovim] K: LSP connect failed: {}", e);
                return;
            }
        }

        if !lsp.is_initialized() {
            if let Err(e) = lsp.initialize(&root_uri) {
                crate::verbose_print!("[godot-neovim] K: LSP init failed: {}", e);
                return;
            }
        }

        // Request hover information
        let line = line_idx as u32;
        let col = col_idx as u32;
        let hover_result = lsp.hover(&uri, line, col);

        match hover_result {
            Ok(Some(hover)) => {
                // Parse hover contents to extract class and member information
                if let Some(query) = Self::parse_hover_for_help(&hover, &word) {
                    crate::verbose_print!(
                        "[godot-neovim] K: LSP hover found - class: {}, member: {:?}, type: {:?}",
                        query.class_name,
                        query.member_name,
                        query.member_type
                    );
                    self.pending_help_query = Some(query);
                } else {
                    crate::verbose_print!("[godot-neovim] K: Could not parse hover for '{}'", word);
                }
            }
            Ok(None) => {
                crate::verbose_print!("[godot-neovim] K: No hover info for '{}'", word);
            }
            Err(e) => {
                crate::verbose_print!("[godot-neovim] K: LSP hover error: {}", e);
            }
        }
    }

    /// Parse LSP hover response to extract class/member information for goto_help()
    fn parse_hover_for_help(hover: &lsp_types::Hover, word: &str) -> Option<HelpQuery> {
        use lsp_types::{HoverContents, MarkedString, MarkupContent};

        // Extract the hover content as a string
        let content = match &hover.contents {
            HoverContents::Scalar(marked) => match marked {
                MarkedString::String(s) => s.clone(),
                MarkedString::LanguageString(ls) => ls.value.clone(),
            },
            HoverContents::Array(arr) => arr
                .iter()
                .map(|m| match m {
                    MarkedString::String(s) => s.as_str(),
                    MarkedString::LanguageString(ls) => ls.value.as_str(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            HoverContents::Markup(MarkupContent { value, .. }) => value.clone(),
        };

        crate::verbose_print!("[godot-neovim] K: Parsing hover content: {}", content);

        // Parse patterns from Godot LSP hover response:
        // - "var ClassName.property_name" -> property
        // - "const ClassName.CONSTANT_NAME" -> constant
        // - "func method_name(...) -> Type" -> method (need to find class from context)
        // - "signal signal_name(...)" -> signal
        // - "<Native> class ClassName" -> class

        // Pattern: "var ClassName.member" or "const ClassName.MEMBER"
        // Regex: (var|const)\s+(\w+)\.(\w+)
        if let Some(caps) = Self::match_class_member(&content) {
            let (keyword, class_name, member_name) = caps;
            let member_type = match keyword {
                "var" => HelpMemberType::Property,
                "const" => HelpMemberType::Constant,
                _ => HelpMemberType::Property,
            };
            return Some(HelpQuery {
                class_name,
                member_name: Some(member_name),
                member_type,
            });
        }

        // Pattern: "func ClassName.method_name(" - method
        if content.contains("func ") && content.contains('(') {
            // Extract class name from "func ClassName.method_name" pattern
            if let Some((class_name, method_name)) = Self::match_func_class_method(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(method_name),
                    member_type: HelpMemberType::Method,
                });
            }
            // Fallback: try to extract class from "Defined in" link
            if let Some(class_name) = Self::extract_class_from_defined_in(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(word.to_string()),
                    member_type: HelpMemberType::Method,
                });
            }
        }

        // Pattern: "signal ClassName.signal_name(" or "signal signal_name(" - signal
        if content.contains("signal ") && content.contains('(') {
            // Extract class name from "signal ClassName.signal_name" pattern
            if let Some((class_name, signal_name)) = Self::match_signal_class_member(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(signal_name),
                    member_type: HelpMemberType::Signal,
                });
            }
            // Fallback: try to extract class from "Defined in" link
            if let Some(class_name) = Self::extract_class_from_defined_in(&content) {
                return Some(HelpQuery {
                    class_name,
                    member_name: Some(word.to_string()),
                    member_type: HelpMemberType::Signal,
                });
            }
        }

        // Pattern: "<Native> class ClassName" or just class name
        if content.contains("class ") {
            // Try to extract class name after "class "
            for line in content.lines() {
                if let Some(idx) = line.find("class ") {
                    let rest = &line[idx + 6..];
                    let class_name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !class_name.is_empty() {
                        return Some(HelpQuery {
                            class_name,
                            member_name: None,
                            member_type: HelpMemberType::Class,
                        });
                    }
                }
            }
        }

        None
    }

    /// Match "var ClassName.member" or "const ClassName.MEMBER" pattern
    fn match_class_member(content: &str) -> Option<(&'static str, String, String)> {
        for line in content.lines() {
            let line = line.trim();

            // Check for "var ClassName.member"
            if let Some(rest) = line.strip_prefix("var ") {
                if let Some((class, member)) = rest.split_once('.') {
                    let class_name: String = class
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let member_name: String = member
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !class_name.is_empty() && !member_name.is_empty() {
                        return Some(("var", class_name, member_name));
                    }
                }
            }

            // Check for "const ClassName.MEMBER"
            if let Some(rest) = line.strip_prefix("const ") {
                if let Some((class, member)) = rest.split_once('.') {
                    let class_name: String = class
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let member_name: String = member
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !class_name.is_empty() && !member_name.is_empty() {
                        return Some(("const", class_name, member_name));
                    }
                }
            }
        }
        None
    }

    /// Extract class name from "Defined in [path](uri)" pattern
    fn extract_class_from_defined_in(content: &str) -> Option<String> {
        // Look for "Defined in [filename.gd]" and extract class from path
        // Native classes: look for res://... path or builtin class reference

        for line in content.lines() {
            // Pattern: "Defined in [path/ClassName.gd]"
            if line.contains("Defined in") {
                // Extract filename from markdown link [filename](uri) or just [filename]
                if let Some(start) = line.find('[') {
                    if let Some(end) = line[start..].find(']') {
                        let path = &line[start + 1..start + end];
                        // Extract class name from filename (e.g., "node.gd" -> "Node")
                        if let Some(filename) = path.split('/').next_back() {
                            if let Some(name) = filename.strip_suffix(".gd") {
                                // Convert snake_case to PascalCase for class name
                                let class_name = Self::to_pascal_case(name);
                                if !class_name.is_empty() {
                                    return Some(class_name);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback: Look for common native class patterns in the content
        // The hover might mention the class in the description
        let native_classes = [
            "Node",
            "Node2D",
            "Node3D",
            "Control",
            "Sprite2D",
            "Sprite3D",
            "Camera2D",
            "Camera3D",
            "CharacterBody2D",
            "CharacterBody3D",
            "RigidBody2D",
            "RigidBody3D",
            "Area2D",
            "Area3D",
            "CollisionShape2D",
            "CollisionShape3D",
            "AnimationPlayer",
            "AudioStreamPlayer",
            "Timer",
            "Label",
            "Button",
            "LineEdit",
            "TextEdit",
            "Panel",
            "Container",
            "HBoxContainer",
            "VBoxContainer",
            "GridContainer",
            "ScrollContainer",
            "TabContainer",
            "Resource",
            "PackedScene",
            "Texture2D",
            "Mesh",
            "Material",
            "Shader",
            "Script",
            "GDScript",
            "Object",
            "RefCounted",
            "Vector2",
            "Vector3",
            "Vector4",
            "Color",
            "Rect2",
            "Transform2D",
            "Transform3D",
            "Basis",
            "Quaternion",
            "AABB",
            "Plane",
            "Array",
            "Dictionary",
            "String",
            "StringName",
            "NodePath",
            "Signal",
            "Callable",
            "PackedByteArray",
            "PackedInt32Array",
            "PackedInt64Array",
            "PackedFloat32Array",
            "PackedFloat64Array",
            "PackedStringArray",
            "PackedVector2Array",
            "PackedVector3Array",
            "PackedColorArray",
            "Input",
            "InputEvent",
            "InputEventKey",
            "InputEventMouse",
            "InputEventMouseButton",
            "InputEventMouseMotion",
            "OS",
            "Engine",
            "ProjectSettings",
            "EditorInterface",
            "EditorPlugin",
            "SceneTree",
            "Viewport",
            "Window",
            "DisplayServer",
            "RenderingServer",
            "PhysicsServer2D",
            "PhysicsServer3D",
            "NavigationServer2D",
            "NavigationServer3D",
            "AudioServer",
            "Time",
            "Performance",
            "Geometry2D",
            "Geometry3D",
            "ResourceLoader",
            "ResourceSaver",
            "FileAccess",
            "DirAccess",
            "JSON",
            "XMLParser",
            "RegEx",
            "Tween",
            "AnimationTree",
            "AnimationNodeStateMachine",
            "AnimationNodeBlendTree",
            "AnimatedSprite2D",
            "AnimatedSprite3D",
            "TileMap",
            "TileSet",
            "CanvasItem",
            "CanvasLayer",
            "ParallaxBackground",
            "ParallaxLayer",
            "PathFollow2D",
            "PathFollow3D",
            "Path2D",
            "Path3D",
            "Curve",
            "Curve2D",
            "Curve3D",
            "Gradient",
            "GradientTexture1D",
            "GradientTexture2D",
            "Image",
            "ImageTexture",
            "AtlasTexture",
            "CompressedTexture2D",
            "Environment",
            "WorldEnvironment",
            "DirectionalLight3D",
            "OmniLight3D",
            "SpotLight3D",
            "Sky",
            "ProceduralSkyMaterial",
            "PhysicalSkyMaterial",
            "PanoramaSkyMaterial",
            "ShaderMaterial",
            "StandardMaterial3D",
            "ORMMaterial3D",
            "BaseMaterial3D",
            "HTTPRequest",
            "HTTPClient",
            "StreamPeer",
            "StreamPeerTCP",
            "StreamPeerTLS",
            "PacketPeer",
            "PacketPeerUDP",
            "TCPServer",
            "UDPServer",
            "WebSocketPeer",
            "MultiplayerAPI",
            "MultiplayerPeer",
            "ENetMultiplayerPeer",
            "WebSocketMultiplayerPeer",
            "Thread",
            "Mutex",
            "Semaphore",
        ];

        for class in native_classes {
            // Check if content mentions this class (case-sensitive)
            if content.contains(class) {
                return Some(class.to_string());
            }
        }

        None
    }

    /// Convert snake_case to PascalCase
    fn to_pascal_case(s: &str) -> String {
        s.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                }
            })
            .collect()
    }

    /// Extract class name and method name from "func ClassName.method_name(...)" pattern
    fn match_func_class_method(content: &str) -> Option<(String, String)> {
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("func ") {
                if let Some((class_part, method_part)) = rest.split_once('.') {
                    let class_name: String = class_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let method_name: String = method_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();

                    if !class_name.is_empty() && !method_name.is_empty() {
                        return Some((class_name, method_name));
                    }
                }
            }
        }
        None
    }

    /// Match "signal ClassName.signal_name(" pattern
    fn match_signal_class_member(content: &str) -> Option<(String, String)> {
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("signal ") {
                if let Some((class_part, signal_part)) = rest.split_once('.') {
                    let class_name: String = class_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let signal_name: String = signal_part
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();

                    if !class_name.is_empty() && !signal_name.is_empty() {
                        return Some((class_name, signal_name));
                    }
                }
            }
        }
        None
    }
}
