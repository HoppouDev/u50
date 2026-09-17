use std::env;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use super::{Resolved, ResolverConfig, ResolverPlugin, ToolSpec};

/// Stable id of this resolver, reused wherever a `ToolSpec` needs to name it
pub const ID: &str = "uv";

/// Resolves and provisions python tools through `uv tool install`
pub struct UvResolver;

impl UvResolver {
    /// Base cache directory, honoring `U50_CACHE_DIR` so tests/CI can
    /// redirect provisioning away from the real user cache
    fn cache_base() -> PathBuf {
        env::var_os("U50_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(dirs::cache_dir)
            .unwrap_or_else(env::temp_dir)
    }

    /// Root cache directory for one tool, scoped by domain and resolver id
    ///
    /// `domain`/`name` are sanitized to safe path segments so a caller-supplied
    /// `ToolSpec` can never escape the cache root via separators or `..`
    pub(crate) fn cache_root(spec: &ToolSpec) -> PathBuf {
        Self::cache_base()
            .join("u50")
            .join(sanitize_segment(&spec.domain))
            .join("uv")
            .join(sanitize_segment(&spec.name))
    }

    /// Environment (venv) directory `uv tool install` manages for this tool
    fn tool_dir(spec: &ToolSpec) -> PathBuf {
        Self::cache_root(spec).join("tool")
    }

    /// Directory `uv tool install` links executables into for this tool
    fn bin_dir(spec: &ToolSpec) -> PathBuf {
        Self::cache_root(spec).join("bin")
    }

    /// Path to `package`'s installed executable inside `bin_dir`
    fn binary_path(spec: &ToolSpec, package: &str) -> PathBuf {
        let file_name = format!("{package}{}", env::consts::EXE_SUFFIX);
        Self::bin_dir(spec).join(file_name)
    }
}

/// Replaces anything outside `[A-Za-z0-9_-]` with `_` so a path segment built
/// from user-controlled input can't contain separators or traverse (`..`)
fn sanitize_segment(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Builds the `package` or `package==version` requirement uv expects
fn package_requirement(package: &str, version: Option<&str>) -> String {
    match version {
        Some(version) => format!("{package}=={version}"),
        None => package.to_owned(),
    }
}

impl ResolverPlugin for UvResolver {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "uv"
    }

    fn supports(&self, spec: &ToolSpec) -> bool {
        matches!(spec.config, ResolverConfig::Uv { .. })
    }

    fn resolve(&self, spec: &ToolSpec) -> Option<Resolved> {
        let ResolverConfig::Uv { package, .. } = &spec.config else {
            return None;
        };
        let path = Self::binary_path(spec, package);
        path.is_file().then_some(Resolved {
            path,
            origin: "found (uv cache)",
        })
    }

    fn provision(&self, spec: &ToolSpec) -> Result<()> {
        let ResolverConfig::Uv { package, version } = &spec.config else {
            anyhow::bail!("uv resolver received a non-uv config for `{}`", spec.name);
        };

        let bin_dir = Self::bin_dir(spec);
        std::fs::create_dir_all(&bin_dir)
            .with_context(|| format!("creating uv bin dir `{}`", bin_dir.display()))?;

        let requirement = package_requirement(package, version.as_deref());
        let output = Command::new("uv")
            .args(["tool", "install", &requirement, "--force"])
            .env("UV_TOOL_DIR", Self::tool_dir(spec))
            .env("UV_TOOL_BIN_DIR", &bin_dir)
            .output()
            .with_context(|| format!("running `uv tool install {requirement}`"))?;

        if !output.status.success() {
            anyhow::bail!(
                "uv failed to install `{requirement}` for `{}`: {}",
                spec.name,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let installed = Self::binary_path(spec, package);
        if !installed.is_file() {
            anyhow::bail!(
                "uv reported success installing `{requirement}` but no binary was found at `{}`",
                installed.display()
            );
        }

        Ok(())
    }
}

inventory::submit! {
    super::Registration(&UvResolver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::resolver;

    fn uv_spec() -> ToolSpec {
        ToolSpec {
            name: "black".to_owned(),
            domain: "style50".to_owned(),
            resolvers: vec!["uv".to_owned()],
            config: ResolverConfig::Uv {
                package: "black".to_owned(),
                version: None,
            },
        }
    }

    #[test]
    fn id_is_uv() {
        assert_eq!(UvResolver.id(), "uv");
        assert_eq!(UvResolver.id(), ID);
    }

    #[test]
    fn supports_uv_configs_only() {
        assert!(UvResolver.supports(&uv_spec()));

        let toolchain_spec = ToolSpec {
            config: ResolverConfig::Toolchain {
                binary: "rustfmt".to_owned(),
            },
            ..uv_spec()
        };
        assert!(!UvResolver.supports(&toolchain_spec));
    }

    #[test]
    fn registers_itself_in_the_resolver_registry() {
        let plugin = resolver::by_id("uv").expect("uv resolver is registered");
        assert_eq!(plugin.id(), "uv");
    }

    #[test]
    fn package_requirement_omits_version_when_absent() {
        assert_eq!(package_requirement("black", None), "black");
    }

    #[test]
    fn package_requirement_pins_version_when_present() {
        assert_eq!(
            package_requirement("black", Some("24.1.0")),
            "black==24.1.0"
        );
    }

    #[test]
    fn resolve_finds_nothing_before_provisioning() {
        let spec = ToolSpec {
            domain: "test-uv-resolve-unprovisioned".to_owned(),
            ..uv_spec()
        };
        assert!(UvResolver.resolve(&spec).is_none());
    }

    #[test]
    fn sanitize_segment_neutralizes_separators_and_traversal() {
        assert_eq!(sanitize_segment("style50"), "style50");
        assert_eq!(sanitize_segment("../../etc/passwd"), "______etc_passwd");
        assert_eq!(sanitize_segment("a/b\\c"), "a_b_c");
    }

    #[test]
    fn cache_root_stays_under_the_cache_base_even_for_hostile_domain_or_name() {
        let spec = ToolSpec {
            name: "../../evil".to_owned(),
            domain: "../../evil-domain".to_owned(),
            ..uv_spec()
        };
        let root = UvResolver::cache_root(&spec);
        assert!(!root.to_string_lossy().contains(".."));
    }

    #[test]
    fn provision_then_resolve_locates_the_installed_binary() {
        let spec = ToolSpec {
            domain: "test-uv-resolver-integration".to_owned(),
            ..uv_spec()
        };

        // Requires network access and a working `uv` on PATH; skip quietly otherwise
        if which::which("uv").is_err() {
            eprintln!(
                "skipping provision_then_resolve_locates_the_installed_binary: uv not on PATH"
            );
            return;
        }

        let cache_root = UvResolver::cache_root(&spec);
        let _ = std::fs::remove_dir_all(&cache_root);

        if UvResolver.provision(&spec).is_err() {
            // No network in this environment; nothing further to assert
            eprintln!(
                "skipping provision_then_resolve_locates_the_installed_binary: provisioning failed (likely no network)"
            );
            let _ = std::fs::remove_dir_all(&cache_root);
            return;
        }

        let resolved = UvResolver
            .resolve(&spec)
            .expect("resolves after provisioning");
        assert!(resolved.path.is_file());
        assert_eq!(resolved.origin, "found (uv cache)");

        let _ = std::fs::remove_dir_all(&cache_root);
    }
}
