//! `ValgrindCapability`: resolves a pinned prebuilt binary via the
//! download resolver (no system reliance; skip-with-guidance on
//! unsupported platforms) (Phase 8).

use anyhow::Result;

#[allow(dead_code)] // scaffolding for the capability layer
/// The pinned valgrind build per platform (conda-forge; URLs + SHA-256
/// to be pinned at integration time — the download resolver validates
/// checksums and the runtime self-check catches kernel mismatches).
pub const VALGRIND_PLATFORMS: &[(&str, &str, &str, &str)] = &[
    // (platform, url, sha256, binary_path)
    ("linux-64", "PENDING", "PENDING", "bin/valgrind"),
    ("linux-aarch64", "PENDING", "PENDING", "bin/valgrind"),
    ("linux-ppc64le", "PENDING", "PENDING", "bin/valgrind"),
    ("osx-64", "PENDING", "PENDING", "bin/valgrind"),
];

#[allow(dead_code)] // scaffolding for the capability layer
/// Provisions valgrind via the download resolver.
///
/// # Errors
/// Returns a clear error when the platform is unsupported or the
/// download/checksum fails.
pub fn ensure_valgrind() -> Result<()> {
    let platform = u50_tools::resolver::host_platform();
    let entry = VALGRIND_PLATFORMS
        .iter()
        .find(|(p, _, _, _)| *p == platform)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "valgrind has no build for this platform ({platform}); valgrind-decorated checks skip with guidance"
            )
        })?;
    if entry.1 == "PENDING" {
        anyhow::bail!(
            "valgrind pinned URL not yet configured; install with your system package manager (apt/dnf/brew install valgrind)"
        );
    }
    // The download resolver handles download + checksum + placement.
    Ok(())
}

#[allow(dead_code)] // scaffolding for the capability layer
/// Whether valgrind is available on this platform.
#[must_use]
pub fn is_supported() -> bool {
    VALGRIND_PLATFORMS
        .iter()
        .any(|(p, url, _, _)| *p == u50_tools::resolver::host_platform() && *url != "PENDING")
}
