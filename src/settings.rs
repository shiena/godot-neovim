use godot::classes::{EditorInterface, EditorSettings};
use godot::prelude::*;
use std::path::Path;
use std::process::{Command, Output};

const SETTING_NEOVIM_PATH: &str = "godot_neovim/neovim_executable_path";
const SETTING_NEOVIM_CLEAN: &str = "godot_neovim/neovim_clean";
const SETTING_TIMEOUTLEN: &str = "godot_neovim/timeoutlen";

/// Default timeout for multi-key sequences (matches Neovim's default)
pub const DEFAULT_TIMEOUTLEN_MS: i64 = 1000;

/// Result of validating Neovim executable path
#[derive(Debug, Clone)]
pub enum ValidationResult {
    Valid { version: String },
    NotFound,
    NotExecutable,
    InvalidVersion { error: String },
}

impl ValidationResult {
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidationResult::Valid { .. })
    }
}

/// Initialize plugin settings in EditorSettings
pub fn initialize_settings() {
    let editor = EditorInterface::singleton();
    let Some(mut settings) = editor.get_editor_settings() else {
        godot_error!("[godot-neovim] Failed to get EditorSettings");
        return;
    };

    // Add setting if it doesn't exist
    if !settings.has_setting(SETTING_NEOVIM_PATH) {
        let default_path = get_default_neovim_path();
        settings.set_setting(SETTING_NEOVIM_PATH, &Variant::from(default_path));
    }

    // Set initial value metadata (p_basic=true: visible by default)
    let default_path = get_default_neovim_path();
    settings.set_initial_value(SETTING_NEOVIM_PATH, &Variant::from(default_path), true);

    // Add property info for better UI
    #[allow(deprecated)]
    let mut property_info = Dictionary::new();
    property_info.set("name", SETTING_NEOVIM_PATH);
    property_info.set("type", VariantType::STRING.ord());
    property_info.set("hint", godot::global::PropertyHint::GLOBAL_FILE.ord());
    property_info.set("hint_string", get_file_filter());

    settings.add_property_info(&property_info);

    // Add neovim_clean setting if it doesn't exist
    // Note: Only set initial value if setting doesn't exist to preserve user's value
    if !settings.has_setting(SETTING_NEOVIM_CLEAN) {
        settings.set_setting(SETTING_NEOVIM_CLEAN, &Variant::from(false));
        // Set initial value only for new settings (p_basic=true: visible by default)
        settings.set_initial_value(SETTING_NEOVIM_CLEAN, &Variant::from(false), true);
    }

    // Add property info for neovim_clean (checkbox)
    #[allow(deprecated)]
    let mut clean_info = Dictionary::new();
    clean_info.set("name", SETTING_NEOVIM_CLEAN);
    clean_info.set("type", VariantType::BOOL.ord());

    settings.add_property_info(&clean_info);

    // Add timeoutlen setting if it doesn't exist (advanced setting)
    if !settings.has_setting(SETTING_TIMEOUTLEN) {
        settings.set_setting(SETTING_TIMEOUTLEN, &Variant::from(DEFAULT_TIMEOUTLEN_MS));
    }

    // Set initial value for timeoutlen (p_basic=false: advanced setting, hidden by default)
    settings.set_initial_value(
        SETTING_TIMEOUTLEN,
        &Variant::from(DEFAULT_TIMEOUTLEN_MS),
        false,
    );

    // Add property info for timeoutlen (integer with range)
    #[allow(deprecated)]
    let mut timeoutlen_info = Dictionary::new();
    timeoutlen_info.set("name", SETTING_TIMEOUTLEN);
    timeoutlen_info.set("type", VariantType::INT.ord());
    timeoutlen_info.set("hint", godot::global::PropertyHint::RANGE.ord());
    timeoutlen_info.set("hint_string", "0,10000,100"); // min, max, step

    settings.add_property_info(&timeoutlen_info);

    crate::verbose_print!(
        "[godot-neovim] Settings initialized. Neovim path: {}, Clean: {}, Timeoutlen: {}ms",
        get_neovim_path(),
        get_neovim_clean(),
        get_timeoutlen()
    );
}

/// Get platform-specific default Neovim path
fn get_default_neovim_path() -> GString {
    #[cfg(target_os = "windows")]
    {
        GString::from("nvim.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        GString::from("nvim")
    }
}

/// Get platform-specific file filter for file dialog
fn get_file_filter() -> GString {
    #[cfg(target_os = "windows")]
    {
        GString::from("*.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        GString::from("*")
    }
}

/// Get the configured Neovim executable path
pub fn get_neovim_path() -> String {
    let editor = EditorInterface::singleton();
    let Some(settings) = editor.get_editor_settings() else {
        return get_default_neovim_path().to_string();
    };

    if settings.has_setting(SETTING_NEOVIM_PATH) {
        let value = settings.get_setting(SETTING_NEOVIM_PATH);
        if let Ok(path) = value.try_to::<GString>() {
            let path_str = path.to_string();
            if !path_str.is_empty() {
                return path_str;
            }
        }
    }

    get_default_neovim_path().to_string()
}

/// Get the configured neovim clean mode
pub fn get_neovim_clean() -> bool {
    let editor = EditorInterface::singleton();
    let Some(settings) = editor.get_editor_settings() else {
        return false;
    };

    if settings.has_setting(SETTING_NEOVIM_CLEAN) {
        let value = settings.get_setting(SETTING_NEOVIM_CLEAN);
        if let Ok(clean) = value.try_to::<bool>() {
            return clean;
        }
    }

    false
}

/// Get the configured timeoutlen (multi-key sequence timeout in milliseconds)
pub fn get_timeoutlen() -> u64 {
    let editor = EditorInterface::singleton();
    let Some(settings) = editor.get_editor_settings() else {
        return DEFAULT_TIMEOUTLEN_MS as u64;
    };

    if settings.has_setting(SETTING_TIMEOUTLEN) {
        let value = settings.get_setting(SETTING_TIMEOUTLEN);
        if let Ok(timeout) = value.try_to::<i64>() {
            // Clamp to valid range (0 to 10000ms)
            return timeout.clamp(0, 10000) as u64;
        }
    }

    DEFAULT_TIMEOUTLEN_MS as u64
}

/// Validate the Neovim executable path
pub fn validate_neovim_path(path: &str) -> ValidationResult {
    if path.is_empty() {
        return ValidationResult::NotFound;
    }

    // Check if it's an absolute path and file exists
    let path_obj = Path::new(path);
    if path_obj.is_absolute() && !path_obj.exists() {
        return ValidationResult::NotFound;
    }

    // Try to execute nvim --version to validate
    match run_nvim_version(path) {
        Ok(output) => {
            if output.status.success() {
                let version_output = String::from_utf8_lossy(&output.stdout);
                // Extract first line which contains version
                let version = version_output
                    .lines()
                    .next()
                    .unwrap_or("Unknown version")
                    .to_string();
                ValidationResult::Valid { version }
            } else {
                let error = String::from_utf8_lossy(&output.stderr).to_string();
                ValidationResult::InvalidVersion { error }
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                ValidationResult::NotFound
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                ValidationResult::NotExecutable
            } else {
                ValidationResult::InvalidVersion {
                    error: e.to_string(),
                }
            }
        }
    }
}

/// Validate the current configured path and print result
pub fn validate_current_path() -> ValidationResult {
    let path = get_neovim_path();
    let result = validate_neovim_path(&path);

    match &result {
        ValidationResult::Valid { version } => {
            crate::verbose_print!("[godot-neovim] Neovim validated: {}", version);
        }
        ValidationResult::NotFound => {
            godot_error!(
                "[godot-neovim] Neovim not found at '{}'. Please check the path in Editor Settings.",
                path
            );
        }
        ValidationResult::NotExecutable => {
            godot_error!(
                "[godot-neovim] Neovim at '{}' is not executable. Please check permissions.",
                path
            );
        }
        ValidationResult::InvalidVersion { error } => {
            godot_error!("[godot-neovim] Invalid Neovim at '{}': {}", path, error);
        }
    }

    result
}

/// Watch for settings changes and validate on change
pub fn on_settings_changed(settings: &Gd<EditorSettings>) {
    if settings.has_setting(SETTING_NEOVIM_PATH) {
        let value = settings.get_setting(SETTING_NEOVIM_PATH);
        if let Ok(path) = value.try_to::<GString>() {
            let path_str = path.to_string();
            let result = validate_neovim_path(&path_str);

            match &result {
                ValidationResult::Valid { version } => {
                    crate::verbose_print!("[godot-neovim] Neovim path validated: {}", version);
                }
                ValidationResult::NotFound => {
                    godot_warn!(
                        "[godot-neovim] Neovim not found at '{}'. The path will be used but may fail at runtime.",
                        path_str
                    );
                }
                ValidationResult::NotExecutable => {
                    godot_warn!("[godot-neovim] Neovim at '{}' is not executable.", path_str);
                }
                ValidationResult::InvalidVersion { error } => {
                    godot_warn!(
                        "[godot-neovim] Neovim validation failed at '{}': {}",
                        path_str,
                        error
                    );
                }
            }
        }
    }
}

/// Run nvim --version with platform-specific settings
fn run_nvim_version(path: &str) -> std::io::Result<Output> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        Command::new(path)
            .arg("--version")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new(path).arg("--version").output()
    }
}
