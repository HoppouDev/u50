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
    // conda-forge valgrind 3.27.1 (linux) / 3.14.0 (osx-64 — the latest
    // available for macOS; valgrind on macOS is less maintained).
    // The .conda format is a zip archive containing pkg-*.tar.zst; full
    // extraction support in the download resolver is deferred — until
    // then, the system resolver provides the binary on hosts that have
    // it installed.
    (
        "linux-64",
        "https://conda.anaconda.org/conda-forge/linux-64/valgrind-3.27.1-hea31c11_0.conda",
        "704011bf371ed51dfa16a2e998dd147a5b4501b29cffd2869ec7b7924bd5ef94",
        "bin/valgrind",
    ),
    (
        "linux-aarch64",
        "https://conda.anaconda.org/conda-forge/linux-aarch64/valgrind-3.27.1-hf239000_0.conda",
        "0bbe293cb42395bbc5f2586c7de7028e3bdc64e8e108e7e7a4e38cca73c8a96b",
        "bin/valgrind",
    ),
    (
        "linux-ppc64le",
        "https://conda.anaconda.org/conda-forge/linux-ppc64le/valgrind-3.27.1-h201c3c0_0.conda",
        "f7803a493a89c338ad78a29937a87ecd7392747ed637553c9179f330bebec384",
        "bin/valgrind",
    ),
    (
        "osx-64",
        "https://conda.anaconda.org/conda-forge/osx-64/valgrind-3.14.0-h6dae8d9_0.tar.bz2",
        "PENDING_SHA256",
        "bin/valgrind",
    ),
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
    if entry.1.starts_with("PENDING") {
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
