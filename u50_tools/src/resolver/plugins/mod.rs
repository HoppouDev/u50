//! The built-in resolver plugins: uv (pip packages via the in-process
//! pipeline), toolchain (Rust toolchain binaries), system (discover-
//! only), and download (pinned standalone binaries).

pub(crate) mod download;
pub(crate) mod system;
pub(crate) mod toolchain;
pub(crate) mod uv;

use crate::resolver::ResolverPlugin;

/// The built-in resolver plugins, in resolution preference order.
pub fn builtin_plugins() -> Vec<Box<dyn ResolverPlugin>> {
    vec![
        Box::new(uv::UvResolver),
        Box::new(toolchain::ToolchainResolver),
        Box::new(system::SystemResolver),
        Box::new(download::DownloadResolver),
    ]
}
