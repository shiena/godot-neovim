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
            child: None,
            neovim: Arc::new(Mutex::new(None)),
            handler,
            nvim_path,
            clean,
            state,
            has_updates,
            io_handle: None,
            key_input_tx: None,
            key_input_handle: None,
            keys_processed: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            keys_processed_seen: std::sync::atomic::AtomicU64::new(0),
        })
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

        let (io_handle, child) = self.runtime.block_on(async {
            let mut cmd = create_nvim_command(&nvim_path, clean);
            cmd.kill_on_drop(true);

            let (neovim, io_handler, child) = create::new_child_cmd(&mut cmd, handler).await?;

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
                let init_code = r#"
                    -- Add addons path to runtimepath
                    local path = ...
                    vim.opt.runtimepath:append(path)
                    -- Load the godot_neovim module
                    require('godot_neovim')
                    "#;
                neovim
                    .exec_lua(init_code, vec![rmpv::Value::from(path.as_str())])
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
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((io_handler, child))
        })?;

        self.io_handle = Some(io_handle);
        self.child = Some(child);

        // Create unbounded channel for key input (no key drops)
        let (tx, mut rx) = unbounded_channel::<String>();
        self.key_input_tx = Some(tx);

        // Spawn key input processor task
        let neovim_arc = self.neovim.clone();
        let keys_processed = self.keys_processed.clone();
        let key_input_handle = self.runtime.spawn(async move {
            while let Some(keys) = rx.recv().await {
                // Clone the Neovim handle out of the lock so the mutex is not
                // held across RPC awaits. Holding it would block the main
                // thread's block_on(lock) calls while a request is in flight.
                let neovim = {
                    let nvim_lock = neovim_arc.lock().await;
                    nvim_lock.as_ref().cloned()
                };
                if let Some(neovim) = neovim {
                    match neovim.input(&keys).await {
                        // nvim_input returns the number of bytes written.
                        // Ok(0) means the typeahead buffer was full and the key
                        // was silently dropped — do NOT count that as an ack.
                        Ok(n) if n > 0 => {
                            // nvim_input is FUNC_API_FAST: it is answered from
                            // Neovim's fast event context even while the main
                            // loop is hung, so its Ok alone proves nothing.
                            // Follow up with a deferred (non-fast) request that
                            // only the main loop can answer; advance the
                            // processed counter only when it completes. A no-op
                            // key (e.g. `j` at the last line) still acks here,
                            // while a genuine main-loop hang stalls the counter
                            // and lets the plugin's watchdog fire (see #75).
                            //
                            // The ack MUST NOT block this loop: a deferred
                            // request is not answered while Neovim waits for
                            // the rest of a multi-key command (e.g. after the
                            // first `g` of `gg`), so awaiting it inline would
                            // deadlock — the completing key sits unsent in the
                            // channel while the ack waits for that very key.
                            // Run it as a detached task instead.
                            let keys_processed = keys_processed.clone();
                            let ack_nvim = neovim.clone();
                            tokio::spawn(async move {
                                if ack_nvim.eval("1").await.is_ok() {
                                    keys_processed
                                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                }
                            });
                        }
                        Ok(_) => {
                            // Note: Can't use godot_error here (tokio thread)
                            eprintln!(
                                "[godot-neovim] Key '{}' dropped: Neovim typeahead buffer full",
                                keys
                            );
                        }
                        Err(e) => {
                            // Log error but continue processing
                            eprintln!("[godot-neovim] Failed to send key '{}': {}", keys, e);
                        }
                    }
                }
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

    pub fn stop(&mut self) {
        let input_task = self.key_input_handle.take();
        let io_task = self.io_handle.take();
        self.key_input_tx = None;
        if let Some(task) = &input_task {
            task.abort();
        }
        if let Some(task) = &io_task {
            task.abort();
        }
        let mut child = self.child.take();
        self.runtime.block_on(async {
            // Await cancellation so old tasks release their RPC handles.
            if let Some(task) = input_task {
                let _ = task.await;
            }
            if let Some(task) = io_task {
                let _ = task.await;
            }
            self.neovim.lock().await.take();
            if let Some(child) = child.as_mut() {
                if let Err(error) = child.kill().await {
                    eprintln!("[godot-neovim] Failed to stop Neovim: {error}");
                }
            }
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    #[test]
    fn stop_waits_for_tasks_and_child_and_is_idempotent() {
        let runtime = Builder::new_current_thread().enable_all().build().unwrap();
        let nvim_path = std::env::var("NVIMRS_TEST_BIN").unwrap_or_else(|_| "nvim".into());
        let mut child = runtime.block_on(async {
            create_nvim_command(&nvim_path, true)
                .kill_on_drop(true)
                .spawn()
                .unwrap()
        });
        assert!(child.try_wait().unwrap().is_none());
        let mut stdout = child.stdout.take().unwrap();
        let handler = NeovimHandler::new();
        let mut client = NeovimClient {
            runtime,
            child: Some(child),
            neovim: Arc::new(Mutex::new(None)),
            state: handler.get_state(),
            has_updates: handler.get_updates_flag(),
            handler,
            nvim_path,
            clean: true,
            io_handle: None,
            key_input_tx: None,
            key_input_handle: None,
            keys_processed: Arc::new(AtomicU64::new(0)),
            keys_processed_seen: AtomicU64::new(0),
        };
        struct MarkDropped(Arc<AtomicU64>);
        impl Drop for MarkDropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicU64::new(0));
        let guard = MarkDropped(dropped.clone());
        client.key_input_handle = Some(client.runtime.spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        }));
        let guard = MarkDropped(dropped.clone());
        client.io_handle = Some(client.runtime.spawn(async move {
            let _guard = guard;
            std::future::pending().await
        }));
        client.stop();
        assert_eq!(dropped.load(Ordering::SeqCst), 2);
        assert!(client.child.is_none());
        client.runtime.block_on(async {
            let mut output = Vec::new();
            tokio::time::timeout(Duration::from_secs(3), stdout.read_to_end(&mut output))
                .await
                .expect("The stopped process must close stdout")
                .unwrap();
        });
        client.stop();
    }
}
