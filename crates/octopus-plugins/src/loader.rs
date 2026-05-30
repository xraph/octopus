//! Plugin loader for dynamic plugins using `libloading`.
//!
//! Loads shared libraries (`.so` / `.dylib` / `.dll`) at runtime,
//! validates the ABI version, and returns boxed [`Plugin`] trait objects.

use crate::abi::PLUGIN_ABI_VERSION;
use crate::traits::Plugin;
use libloading::{Library, Symbol};
use octopus_core::{Error, Result};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use tracing::{debug, error, info, warn};

/// Dynamic plugin loader.
///
/// Keeps loaded [`Library`] handles alive for the lifetime of the loader
/// so that symbol references remain valid.
pub struct PluginLoader {
    /// Loaded libraries -- kept alive to prevent the OS from unloading them.
    libraries: Vec<Library>,
    /// Optional search paths used by [`load_directory`](Self::load_directory).
    search_paths: Vec<PathBuf>,
}

impl std::fmt::Debug for PluginLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginLoader")
            .field("loaded_libraries", &self.libraries.len())
            .field("search_paths", &self.search_paths)
            .finish()
    }
}

impl PluginLoader {
    /// Create a new plugin loader with no search paths.
    pub fn new() -> Self {
        Self {
            libraries: Vec::new(),
            search_paths: Vec::new(),
        }
    }

    /// Create a plugin loader with the given search paths.
    pub fn with_search_paths(paths: Vec<PathBuf>) -> Self {
        Self {
            libraries: Vec::new(),
            search_paths: paths,
        }
    }

    /// Load a plugin from a shared library file.
    ///
    /// The library must export the symbols defined by the
    /// [`octopus_declare_plugin!`](crate::octopus_declare_plugin) macro.
    ///
    /// # Safety
    ///
    /// Loading arbitrary shared libraries is inherently unsafe. The library
    /// **must** be compiled with a compatible Rust toolchain and expose the
    /// expected C-ABI symbols. Mismatched ABIs will be caught by the version
    /// check, but other forms of UB (e.g. wrong struct layouts) cannot be
    /// detected at runtime.
    #[allow(unsafe_code)]
    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<Box<dyn Plugin>> {
        let path = path.as_ref();
        debug!(path = %path.display(), "Loading plugin from shared library");

        // 1. Load the shared library.
        // SAFETY: The caller is responsible for providing a valid shared
        // library that exports the required symbols.
        let library = unsafe { Library::new(path) }.map_err(|e| Error::Plugin {
            plugin: path.display().to_string(),
            message: format!("Failed to load shared library: {e}"),
        })?;

        // 2. Validate ABI version.
        let abi_version: u32 = unsafe {
            let func: Symbol<'_, unsafe extern "C" fn() -> u32> =
                library.get(b"octopus_plugin_abi_version").map_err(|e| {
                    Error::Plugin {
                        plugin: path.display().to_string(),
                        message: format!(
                            "Library does not export 'octopus_plugin_abi_version': {e}"
                        ),
                    }
                })?;
            func()
        };

        if abi_version != PLUGIN_ABI_VERSION {
            return Err(Error::Plugin {
                plugin: path.display().to_string(),
                message: format!(
                    "ABI version mismatch: plugin has v{abi_version}, expected v{PLUGIN_ABI_VERSION}"
                ),
            });
        }

        // 3. Read plugin name (for logging).
        let plugin_name: String = unsafe {
            let func: Symbol<'_, unsafe extern "C" fn() -> *const c_char> =
                library.get(b"octopus_plugin_name").map_err(|e| {
                    Error::Plugin {
                        plugin: path.display().to_string(),
                        message: format!("Library does not export 'octopus_plugin_name': {e}"),
                    }
                })?;

            let ptr = func();
            if ptr.is_null() {
                return Err(Error::Plugin {
                    plugin: path.display().to_string(),
                    message: "octopus_plugin_name returned null".to_string(),
                });
            }
            CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned()
        };

        // 4. Read plugin version (for logging).
        let plugin_version: String = unsafe {
            let func: Symbol<'_, unsafe extern "C" fn() -> *const c_char> =
                library.get(b"octopus_plugin_version").map_err(|e| {
                    Error::Plugin {
                        plugin: plugin_name.clone(),
                        message: format!(
                            "Library does not export 'octopus_plugin_version': {e}"
                        ),
                    }
                })?;

            let ptr = func();
            if ptr.is_null() {
                return Err(Error::Plugin {
                    plugin: plugin_name.clone(),
                    message: "octopus_plugin_version returned null".to_string(),
                });
            }
            CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned()
        };

        // 5. Create the plugin instance.
        // SAFETY: `octopus_create_plugin` must return a valid, heap-allocated
        // `Box<dyn Plugin>` pointer (via `Box::into_raw`).
        let plugin: Box<dyn Plugin> = unsafe {
            let func: Symbol<'_, unsafe extern "C" fn() -> *mut dyn Plugin> =
                library.get(b"octopus_create_plugin").map_err(|e| {
                    Error::Plugin {
                        plugin: plugin_name.clone(),
                        message: format!(
                            "Library does not export 'octopus_create_plugin': {e}"
                        ),
                    }
                })?;

            let raw = func();
            if raw.is_null() {
                return Err(Error::Plugin {
                    plugin: plugin_name.clone(),
                    message: "octopus_create_plugin returned null".to_string(),
                });
            }
            Box::from_raw(raw)
        };

        // 6. Keep the library handle alive.
        self.libraries.push(library);

        info!(
            plugin = %plugin_name,
            version = %plugin_version,
            path = %path.display(),
            "Successfully loaded dynamic plugin"
        );

        Ok(plugin)
    }

    /// Load all plugins found in a directory.
    ///
    /// Scans for platform-appropriate shared library files (`.so` on Linux,
    /// `.dylib` on macOS, `.dll` on Windows). Errors for individual files
    /// are logged but do not prevent other plugins from loading.
    pub fn load_directory<P: AsRef<Path>>(&mut self, dir: P) -> Result<Vec<Box<dyn Plugin>>> {
        let dir = dir.as_ref();
        debug!(dir = %dir.display(), "Loading plugins from directory");

        if !dir.is_dir() {
            return Err(Error::Plugin {
                plugin: "loader".to_string(),
                message: format!("Plugin directory does not exist: {}", dir.display()),
            });
        }

        let extensions = platform_library_extensions();
        let mut plugins = Vec::new();

        let entries = std::fs::read_dir(dir).map_err(|e| Error::Plugin {
            plugin: "loader".to_string(),
            message: format!("Failed to read plugin directory {}: {e}", dir.display()),
        })?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "Failed to read directory entry");
                    continue;
                }
            };

            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let is_plugin = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.contains(&ext))
                .unwrap_or(false);

            if !is_plugin {
                continue;
            }

            match self.load(&path) {
                Ok(plugin) => plugins.push(plugin),
                Err(e) => {
                    error!(
                        path = %path.display(),
                        error = %e,
                        "Failed to load plugin, skipping"
                    );
                }
            }
        }

        info!(
            dir = %dir.display(),
            count = plugins.len(),
            "Loaded plugins from directory"
        );

        Ok(plugins)
    }

    /// Return the number of loaded libraries.
    pub fn loaded_count(&self) -> usize {
        self.libraries.len()
    }

    /// Return the configured search paths.
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the shared library file extensions for the current platform.
fn platform_library_extensions() -> &'static [&'static str] {
    if cfg!(target_os = "linux") {
        &["so"]
    } else if cfg!(target_os = "macos") {
        &["dylib"]
    } else if cfg!(target_os = "windows") {
        &["dll"]
    } else {
        &["so", "dylib", "dll"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_loader_new() {
        let loader = PluginLoader::new();
        assert_eq!(loader.loaded_count(), 0);
        assert!(loader.search_paths().is_empty());
    }

    #[test]
    fn test_loader_with_search_paths() {
        let paths = vec![PathBuf::from("/usr/lib/plugins"), PathBuf::from("/opt/plugins")];
        let loader = PluginLoader::with_search_paths(paths.clone());
        assert_eq!(loader.search_paths(), &paths);
    }

    #[test]
    fn test_load_nonexistent_file_returns_error() {
        let mut loader = PluginLoader::new();
        let result = loader.load("/nonexistent/path/plugin.so");
        assert!(result.is_err());
        match result {
            Err(err) => {
                let err_msg = err.to_string();
                assert!(
                    err_msg.contains("Failed to load shared library"),
                    "Unexpected error message: {err_msg}"
                );
            }
            Ok(_) => panic!("Expected error for nonexistent plugin file"),
        }
    }

    #[test]
    fn test_load_directory_nonexistent_returns_error() {
        let mut loader = PluginLoader::new();
        let result = loader.load_directory("/nonexistent/plugin/dir");
        match result {
            Err(err) => assert!(err.to_string().contains("does not exist")),
            Ok(_) => panic!("Expected error for nonexistent directory"),
        }
    }

    #[test]
    fn test_load_directory_empty_dir() {
        let dir = std::env::temp_dir().join("octopus_test_empty_plugin_dir");
        let _ = std::fs::create_dir_all(&dir);

        let mut loader = PluginLoader::new();
        let plugins = loader.load_directory(&dir).unwrap();
        assert!(plugins.is_empty());

        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_abi_version_constant() {
        assert_eq!(PLUGIN_ABI_VERSION, 1);
    }

    #[test]
    fn test_platform_library_extensions() {
        let exts = platform_library_extensions();
        assert!(!exts.is_empty());

        if cfg!(target_os = "macos") {
            assert!(exts.contains(&"dylib"));
        } else if cfg!(target_os = "linux") {
            assert!(exts.contains(&"so"));
        } else if cfg!(target_os = "windows") {
            assert!(exts.contains(&"dll"));
        }
    }

    #[test]
    fn test_loader_debug_format() {
        let loader = PluginLoader::new();
        let debug_str = format!("{:?}", loader);
        assert!(debug_str.contains("PluginLoader"));
        assert!(debug_str.contains("loaded_libraries"));
    }
}
