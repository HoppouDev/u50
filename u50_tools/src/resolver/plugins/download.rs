//! The download resolver plugin: pinned standalone-binary downloads
//! into the cache (URL + SHA-256 + platform mapping).

use std::path::PathBuf;

use anyhow::Result;
use sha2::Digest as _;

use crate::resolver::{
    PlatformBinary, Resolved, ResolverConfig, ResolverPlugin, ToolSpec, host_platform,
};

/// The download resolver: pinned standalone binaries into
/// `<cache>/<tool>/<version>/<platform>/`.
pub struct DownloadResolver;

impl ResolverPlugin for DownloadResolver {
    fn id(&self) -> &'static str {
        "download"
    }

    fn supports(&self, spec: &ToolSpec) -> bool {
        matches!(spec.config, ResolverConfig::Download { .. })
    }

    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved> {
        let ResolverConfig::Download { platforms } = &spec.config else {
            return None;
        };
        let platform = host_platform();
        let entry = platforms.iter().find(|p| p.platform == platform)?;
        let cache = uv::cache_root_for_spec(spec).join(&spec.name);
        let binary = cache.join(&entry.binary_path);
        if binary.is_file() {
            Some(Resolved { path: binary, origin: "found (cache)" })
        } else {
            None
        }
    }

    fn provision(&self, spec: &ToolSpec) -> Result<()> {
        let ResolverConfig::Download { platforms } = &spec.config else {
            anyhow::bail!("download resolver: not a download tool spec");
        };
        let platform = host_platform();
        let entry = platforms
            .iter()
            .find(|p| p.platform == platform)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} has no build for this platform ({platform}); it skips with guidance on unsupported platforms",
                    spec.name
                )
            })?;
        let cache = cache_dir_for(spec);
        std::fs::create_dir_all(&cache).context("create download cache dir")?;
        let binary = cache.join(&entry.binary_path);
        if binary.is_file() {
            return Ok(()); // idempotent
        }

        // Download + verify + extract (or place) the pinned binary.
        tracing::info!(tool = %spec.name, url = %entry.url, "downloading pinned binary");
        let client = reqwest::blocking::Client::new();
        let bytes = client
            .get(&entry.url)
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.bytes())
            .map_err(|e| anyhow::anyhow!("could not download {}: {e}", entry.url))?;
        let digest = sha2::Sha256::digest(&bytes);
        let actual = digest.iter().fold(String::new(), |mut out, byte| {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
            out
        });
        anyhow::ensure!(
            actual == entry.sha256,
            "checksum mismatch for {}: expected {}, got {}",
            entry.url,
            entry.sha256,
            actual
        );

        // Place the binary (a plain file — archives need per-format
        // extraction, deferred to when a real tool uses this resolver).
        if let Some(parent) = binary.parent() {
            std::fs::create_dir_all(parent).context("create binary dir")?;
        }
        std::fs::write(&binary, &bytes).context("write binary")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mut permissions = std::fs::permissions(&binary)?;
            permissions.set_mode(0o755);
            std::fs::set_permissions(&binary, permissions)?;
        }
        Ok(())
    }
}
