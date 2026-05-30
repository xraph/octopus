//! ABI-safe interface for dynamically loaded plugins.
//!
//! Plugin authors use the [`octopus_declare_plugin!`] macro to export the
//! required C symbols from their shared library. The [`PluginLoader`](crate::loader::PluginLoader)
//! looks for these symbols at load time.

/// Current plugin ABI version. Bump this on any breaking change to the
/// plugin interface so the loader can reject incompatible libraries.
pub const PLUGIN_ABI_VERSION: u32 = 1;

/// Declare a dynamic plugin for the Octopus gateway.
///
/// This macro must be invoked exactly once in the plugin crate's `lib.rs`.
/// It exports the four C-ABI symbols that the loader expects:
///
/// - `octopus_plugin_abi_version` -- returns [`PLUGIN_ABI_VERSION`]
/// - `octopus_plugin_name` -- returns the plugin name as a null-terminated C string
/// - `octopus_plugin_version` -- returns the plugin version as a null-terminated C string
/// - `octopus_create_plugin` -- allocates and returns a heap-allocated [`Plugin`](crate::Plugin) trait object
///
/// # Example
///
/// ```ignore
/// use octopus_plugins::octopus_declare_plugin;
///
/// #[derive(Default)]
/// struct MyPlugin { /* ... */ }
///
/// // impl Plugin for MyPlugin { ... }
///
/// octopus_declare_plugin!(MyPlugin, "my-plugin", "1.0.0");
/// ```
#[macro_export]
macro_rules! octopus_declare_plugin {
    ($plugin_type:ty, $name:expr, $version:expr) => {
        /// Return the ABI version this plugin was compiled against.
        #[no_mangle]
        pub extern "C" fn octopus_plugin_abi_version() -> u32 {
            $crate::abi::PLUGIN_ABI_VERSION
        }

        /// Return the plugin name as a null-terminated C string.
        #[no_mangle]
        pub extern "C" fn octopus_plugin_name() -> *const std::os::raw::c_char {
            concat!($name, "\0").as_ptr() as *const std::os::raw::c_char
        }

        /// Return the plugin version as a null-terminated C string.
        #[no_mangle]
        pub extern "C" fn octopus_plugin_version() -> *const std::os::raw::c_char {
            concat!($version, "\0").as_ptr() as *const std::os::raw::c_char
        }

        /// Create a new plugin instance on the heap and return a raw pointer.
        ///
        /// # Safety
        ///
        /// The caller (the plugin loader) is responsible for eventually
        /// reclaiming the allocation via `Box::from_raw`.
        #[no_mangle]
        pub extern "C" fn octopus_create_plugin() -> *mut dyn $crate::Plugin {
            let plugin = <$plugin_type>::default();
            Box::into_raw(Box::new(plugin))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_version_constant() {
        assert_eq!(PLUGIN_ABI_VERSION, 1);
    }

    #[test]
    fn test_abi_version_is_nonzero() {
        // ABI version should always be a positive integer
        assert!(PLUGIN_ABI_VERSION > 0);
    }
}
