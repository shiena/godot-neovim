//! File type detection for Neovim filetype setting
//!
//! Maps file extensions to Neovim filetype names for proper syntax highlighting
//! and language-specific features.

/// Detect Neovim filetype from file path
///
/// Returns the appropriate Neovim filetype string based on the file extension.
/// Falls back to "text" for unknown extensions.
pub fn detect_filetype(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext.to_lowercase().as_str() {
        // GDScript
        "gd" => "gdscript",

        // Shaders
        "gdshader" | "shader" => "gdshader",

        // Common text files (supported by Godot's TextFile)
        "txt" => "text",
        "md" => "markdown",
        "json" => "json",
        "cfg" | "ini" => "dosini",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "log" => "text",

        // C# (for Godot C# projects)
        "cs" => "cs",

        // Default
        _ => "text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_filetype() {
        assert_eq!(detect_filetype("res://player.gd"), "gdscript");
        assert_eq!(detect_filetype("res://shaders/water.gdshader"), "gdshader");
        assert_eq!(detect_filetype("res://README.md"), "markdown");
        assert_eq!(detect_filetype("res://data.json"), "json");
        assert_eq!(detect_filetype("res://config.cfg"), "dosini");
        assert_eq!(detect_filetype("res://unknown.xyz"), "text");
    }
}
