//! Connection management: new, start, stop

use super::{NeovimClient, NeovimVersion, Writer, LUA_FALLBACK_CODE, NEOVIM_REQUIRED_VERSION};
use crate::neovim::NeovimHandler;
use crate::settings;
use godot::prelude::godot_warn;
use nvim_rs::create::tokio as create;
use nvim_rs::{Neovim, UiAttachOptions};
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::runtime::Builder;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::Mutex;

#[cfg(target_os = "windows")]
use super::CREATE_NO_WINDOW;

impl NeovimClient {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Use current-thread runtime so all tasks run on the same thread
        // This ensures io_handler is processed during block_on calls
        let runtime = Builder::new_current_thread().enable_all().build()?;
        let nvim_path = settings::get_neovim_path();
        let clean = settings::get_neovim_clean();
        let handler = NeovimHandler::new();
        let state = handler.get_state();
        let has_updates = handler.get_updates_flag();
        Ok(Self {
            runtime,
            neovim: Arc::new(Mutex::new(None)),
            handler,
            nvim_path,
            clean,
            state,
            has_updates,
            io_handle: None,
            key_input_tx: None,
            key_input_handle: None,
        })
    }

    /// Update the Neovim executable path
    #[allow(dead_code)]
    pub fn set_neovim_path(&mut self, path: String) {
        self.nvim_path = path;
    }

    /// Get the current Neovim executable path
    #[allow(dead_code)]
    pub fn get_neovim_path(&self) -> &str {
        &self.nvim_path
    }

    /// Start Neovim process and establish connection
    /// If addons_path is provided, loads the Lua plugin from that directory
    pub fn start(
        &mut self,
        addons_path: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let handler = self.handler.clone();
        let neovim_arc = self.neovim.clone();
        let nvim_path = self.nvim_path.clone();
        let clean = self.clean;
        let addons_path_owned = addons_path.map(String::from);

        crate::verbose_print!(
            "[godot-neovim] Starting Neovim: {} (clean={}, addons_path={:?})",
            nvim_path,
            clean,
            addons_path
        );

        let io_handle = self.runtime.block_on(async {
            let mut cmd = create_nvim_command(&nvim_path, clean);

            let (neovim, io_handler, _child) = create::new_child_cmd(&mut cmd, handler).await?;

            // Attach UI to receive redraw events
            // ext_multigrid enables win_viewport events for viewport synchronization
            let mut ui_opts = UiAttachOptions::new();
            ui_opts.set_rgb(true);
            ui_opts.set_linegrid_external(true);
            ui_opts.set_multigrid_external(true);
            neovim
                .ui_attach(80, 24, &ui_opts)
                .await
                .map_err(|e| format!("Failed to attach UI: {}", e))?;

            crate::verbose_print!("[godot-neovim] UI attached successfully");

            // Disable swap files and handle E325 ATTENTION errors in headless mode
            // - noswapfile: Don't create new swap files
            // - shortmess+=A: Suppress swap file warnings
            // - SwapExists autocmd: Auto-select 'edit anyway' if swap exists
            neovim
                .command(
                    "set noswapfile shortmess+=A | autocmd SwapExists * let v:swapchoice = 'e'",
                )
                .await
                .map_err(|e| format!("Failed to configure swapfile handling: {}", e))?;

            // Check Neovim version before storing
            let version = get_neovim_version(&neovim).await;
            let (req_major, req_minor, req_patch) = NEOVIM_REQUIRED_VERSION;

            if let Some(ref ver) = version {
                crate::verbose_print!("[godot-neovim] Neovim version: {}", ver);

                if !ver.meets_requirement(req_major, req_minor, req_patch) {
                    let msg = format!(
                        "Neovim version {} is below minimum required {}.{}.{}. Some features may not work correctly.",
                        ver, req_major, req_minor, req_patch
                    );
                    godot_warn!("[godot-neovim] {}", msg);
                }
            } else {
                crate::verbose_print!("[godot-neovim] Could not determine Neovim version");
            }

            // Initialize godot_neovim Lua module
            // Prefer external plugin if addons_path is provided
            if let Some(ref path) = addons_path_owned {
                // Escape backslashes for Lua string (Windows paths)
                let lua_path = path.replace('\\', "/");
                let init_code = format!(
                    r#"
                    -- Add addons path to runtimepath
                    vim.opt.runtimepath:append("{}")
                    -- Load the godot_neovim module
                    require('godot_neovim')
                    "#,
                    lua_path
                );
                neovim
                    .exec_lua(&init_code, vec![])
                    .await
                    .map_err(|e| format!("Failed to load Lua plugin from {}: {}", path, e))?;
                crate::verbose_print!(
                    "[godot-neovim] Lua module loaded from external file: {}",
                    path
                );
            } else {
                // Fallback to embedded Lua code
                neovim
                    .exec_lua(LUA_FALLBACK_CODE, vec![])
                    .await
                    .map_err(|e| format!("Failed to initialize Lua module: {}", e))?;
                crate::verbose_print!("[godot-neovim] Lua module initialized (embedded fallback)");
            }

            let mut nvim_lock = neovim_arc.lock().await;
            *nvim_lock = Some(neovim);

            crate::verbose_print!("[godot-neovim] Neovim started successfully");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(io_handler)
        })?;

        self.io_handle = Some(io_handle);

        // Create unbounded channel for key input (no key drops)
        let (tx, mut rx) = unbounded_channel::<String>();
        self.key_input_tx = Some(tx);

        // Spawn key input processor task
        let neovim_arc = self.neovim.clone();
        let key_input_handle = self.runtime.spawn(async move {
            while let Some(keys) = rx.recv().await {
                let nvim_lock = neovim_arc.lock().await;
                if let Some(neovim) = nvim_lock.as_ref() {
                    if let Err(e) = neovim.input(&keys).await {
                        // Log error but continue processing
                        // Note: Can't use godot_error here (tokio thread)
                        eprintln!("[godot-neovim] Failed to send key '{}': {}", keys, e);
                    }
                }
                // Release lock before next iteration
                drop(nvim_lock);
            }
        });
        self.key_input_handle = Some(key_input_handle);

        crate::verbose_print!(
            "[godot-neovim] IO handler spawned, has_updates={}",
            self.has_updates.load(std::sync::atomic::Ordering::SeqCst)
        );
        crate::verbose_print!("[godot-neovim] Key input channel initialized (unbounded)");

        Ok(())
    }

    /// Stop Neovim process
    pub fn stop(&mut self) {
        // Abort the key input handler first
        if let Some(handle) = self.key_input_handle.take() {
            handle.abort();
            crate::verbose_print!("[godot-neovim] Key input handler aborted");
        }
        // Clear the key input sender
        self.key_input_tx = None;

        // Abort the IO handler to prevent blocking on read
        if let Some(handle) = self.io_handle.take() {
            handle.abort();
            crate::verbose_print!("[godot-neovim] IO handler aborted");
        }

        // Clear the neovim instance without sending quit command
        // (IO is already aborted, command would timeout anyway)
        let neovim_arc = self.neovim.clone();
        self.runtime.block_on(async {
            let mut nvim_lock = neovim_arc.lock().await;
            nvim_lock.take();
        });
        crate::verbose_print!("[godot-neovim] Neovim stopped");
    }
}

/// Create Neovim command with platform-specific settings
fn create_nvim_command(nvim_path: &str, clean: bool) -> Command {
    // -n: No swap file (prevents E325 ATTENTION errors in headless mode)
    let mut args = vec!["--embed", "--headless", "-n"];
    if clean {
        args.push("--clean");
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut std_cmd = std::process::Command::new(nvim_path);
        std_cmd
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        Command::from(std_cmd)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = Command::new(nvim_path);
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}

/// Get Neovim version from API info
pub(super) async fn get_neovim_version(neovim: &Neovim<Writer>) -> Option<NeovimVersion> {
    // Call nvim_get_api_info to get version information
    let api_info = neovim.get_api_info().await.ok()?;

    // API info is [channel_id, {version: {...}, functions: [...], ...}]
    let info_map = api_info.get(1)?;
    let info_map = info_map.as_map()?;

    // Find version in the map
    for (key, value) in info_map {
        if key.as_str() == Some("version") {
            let version_map = value.as_map()?;
            let mut major = 0u64;
            let mut minor = 0u64;
            let mut patch = 0u64;

            for (vkey, vval) in version_map {
                match vkey.as_str() {
                    Some("major") => major = vval.as_u64().unwrap_or(0),
                    Some("minor") => minor = vval.as_u64().unwrap_or(0),
                    Some("patch") => patch = vval.as_u64().unwrap_or(0),
                    _ => {}
                }
            }

            return Some(NeovimVersion {
                major,
                minor,
                patch,
            });
        }
    }

    None
}
