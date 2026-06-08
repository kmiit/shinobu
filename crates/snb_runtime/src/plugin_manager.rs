use anyhow::Result;
use std::any::Any;
use std::ffi::CStr;
use std::sync::Arc;

use crate::bot::Bot;
use snb_core::context::BotContext;
use snb_core::error::PluginError;
use snb_core::plugin::{PluginCell, SnbPlugin, Version, snb_plugin_abi};

/// Loads and unloads plugin shared libraries (`.so` / `.dylib` / `.dll`).
///
/// Validates ABI compatibility before calling the plugin's `on_load`, and
/// ensures safe teardown order on unload (plugin `on_unload` → drop all
/// trait objects → drop the library handle).
pub struct PluginLoader {
    bot: Arc<Bot>,
}

impl PluginLoader {
    pub fn new(bot: Arc<Bot>) -> Self {
        // Ensure the bot context is set so `log::*!` macros work throughout
        // plugin loading. The runtime should have already called `set_bot`,
        // but we check to avoid overwriting it if already present.
        if !snb_core::context::bot_is_set() {
            snb_core::context::set_bot(bot.clone());
        }
        Self { bot }
    }

    pub fn load_plugin(&self, path: std::path::PathBuf) -> Result<()> {
        let current_plugin_abi = snb_plugin_abi();
        let lib = unsafe { libloading::Library::new(path)? };

        let (ptr, destroy_fn, ffi_abi) = unsafe {
            let create_sym: libloading::Symbol<unsafe extern "C" fn() -> *mut Box<dyn SnbPlugin>> =
                lib.get(b"create_plugin")?;
            let destroy_sym: libloading::Symbol<unsafe extern "C" fn(*mut Box<dyn SnbPlugin>)> =
                lib.get(b"destroy_plugin")?;
            let abi_sym: libloading::Symbol<unsafe extern "C" fn() -> *const std::ffi::c_char> =
                lib.get(b"plugin_abi")?;

            let abi_str = CStr::from_ptr(abi_sym()).to_str()?;
            let abi: Version = abi_str.parse().map_err(|_| PluginError::UnsupportedAbi)?;

            // Check ABI compatibility: major must match, minor must be <= runtime's
            if abi.major != current_plugin_abi.major {
                log::error!(
                    "ABI major version mismatch: plugin={}, runtime={} (incompatible)",
                    abi, current_plugin_abi
                );
                return Err(PluginError::UnsupportedAbi)?;
            }

            if abi.minor > current_plugin_abi.minor {
                log::error!(
                    "ABI minor version too new: plugin={}, runtime={} (plugin needs features not available in runtime)",
                    abi, current_plugin_abi
                );
                return Err(PluginError::UnsupportedAbi)?;
            }

            // Warn on minor or patch differences (backward compatible but potentially outdated)
            if abi.minor < current_plugin_abi.minor {
                log::warn!(
                    "ABI minor version mismatch: plugin={}, runtime={} (plugin built against older ABI, may miss new features)",
                    abi, current_plugin_abi
                );
            }

            if abi.patch != current_plugin_abi.patch {
                log::warn!(
                    "ABI patch version mismatch: plugin={}, runtime={} (compatible but rebuild recommended)",
                    abi, current_plugin_abi
                );
            }

            let ptr = create_sym();
            let destroy_fn = *destroy_sym; // copy the fn pointer (Copy type)
            (ptr, destroy_fn, abi)
        };
        let keep_alive: Box<dyn Any + Send + Sync> = Box::new(lib);
        let mut cell = unsafe { PluginCell::new(ptr, destroy_fn, keep_alive) };

        if cell.abi_version().major != ffi_abi.major {
            log::warn!(
                "Plugin {} ABI major {} does not match plugin_abi export major {}",
                cell.name(),
                cell.abi_version().major,
                ffi_abi.major
            );
            return Err(PluginError::BrokenAbi)?;
        }

        // Refuse a duplicate plugin name before running `on_load`, so a
        // shadowed plugin never touches the registry.
        let name = cell.name().to_string();
        if self.bot.get_plugin(&name).is_some() {
            log::error!(
                "plugin '{}' is already loaded; refusing duplicate",
                name
            );
            return Err(PluginError::DuplicatePlugin)?;
        }

        // Bracket `on_load` so component registrations that hit a name clash are
        // recorded rather than silently overwriting another plugin's entries.
        self.bot.begin_plugin_load();
        cell.on_load(self.bot.clone());
        let conflicts = self.bot.take_plugin_load_conflicts();
        if !conflicts.is_empty() {
            log::error!(
                "refusing plugin '{}': {} name conflict(s): {}",
                name,
                conflicts.len(),
                conflicts.join("; ")
            );
            // Tear down anything this plugin managed to register before the
            // clash, then drop the cell (destroy_plugin → dlclose) — in that
            // order, so no Arc outlives the dylib it points into.
            cell.on_unload();
            self.bot.rollback_plugin_components(&name);
            return Err(PluginError::ComponentConflict)?;
        }

        self.bot.register_plugin(cell);
        Ok(())
    }

    pub fn unload_plugin(&self, name: &str) -> Result<()> {
        if self.bot.unregister_plugin(name) {
            Ok(())
        } else {
            Err(PluginError::UnloadError)?
        }
    }
}
