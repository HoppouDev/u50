//! Setup support: provisions a uv-managed `CPython` and venv inside
//! the u50 cache and installs the missing formatter backends into it
//! in-process (uv library calls - no pip subprocesses), so future runs
//! can resolve the tools from the cache without root or a system-wide
//! install.

mod pins;
mod pipeline;
mod venv;
mod wheels;

use anyhow::{Context, Result, bail};
use tokio::runtime::Runtime;
use uv_preview::Preview;

use crate::format::{cache_dir, locate_tool};
use crate::language::Language;
use pipeline::provision_backends;

/// Computes the distinct missing pip packages, in first-seen language
/// order: iterates [`Language::ALL`], skips languages whose backing tool
/// `is_resolved`, and dedups by pip package (C/C++/Java all share
/// `clang-format`, so they collapse to one entry). Pure decision logic so
/// the missing-backend computation is unit-testable without any
/// provisioning; the uv install path itself is exercised by manual smoke
/// runs (it needs network access).
fn missing_backends(is_resolved: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut missing: Vec<(String, String)> = Vec::new();
    for &language in &Language::ALL {
        let tool = language.required_tool();
        if is_resolved(tool) {
            continue;
        }
        let pip_package = language.pip_package();
        if !missing.iter().any(|(pkg, _)| pkg == pip_package) {
            missing.push((pip_package.to_owned(), tool.to_owned()));
        }
    }
    missing
}

/// The per-backend summary outcome reported by [`setup_missing`].
struct BackendOutcome {
    package: String,
    tool: String,
    /// `None` when the backend installed successfully; the failure
    /// reason otherwise.
    failure: Option<String>,
}

/// Installs missing formatter backends into the cache. Missing tools are
/// detected per language; a uv-managed `CPython` ([`PINNED_PYTHON`]) and a
/// venv at `<cache>/venv` are provisioned if absent, then each missing
/// backend package (plus its hardcoded transitive dependencies, see
/// [`TRANSITIVE_DEPS`]) is resolved, downloaded and unpacked in parallel
/// (one spawned task and one spinner per package) and installed into the
/// venv with `uv-installer`.
///
/// A package counts as installed only when its backing tool is resolvable
/// from the cache bin dir afterwards. Per-package summary lines are
/// printed (`installed: <pkg> (<tool>)` / `failed: <pkg>: <reason>`).
/// Dependency failures are reported through the parent package so it does
/// not double-report.
///
/// The missing-backend decision path is unit-tested in
/// [`missing_backends`]; the uv provisioning path is covered by manual
/// smoke runs (network), not unit tests.
///
/// # Errors
/// Returns an error (CLI exit code 3) when uv provisioning fails or any
/// package failed to install.
pub fn setup_missing() -> Result<()> {
    // Distinct missing pip packages, in first-seen language order.
    let missing = missing_backends(|tool| locate_tool(tool).is_some());

    if missing.is_empty() {
        println!("all formatter backends are already available");
        return Ok(());
    }
    install_backends(&missing, ProgressTarget::Stdout)
}

/// Where provisioning progress lines go: the explicit `u50 --setup`
/// path owns stdout (the report IS the output); the lazy
/// auto-provisioning path runs mid-style-check, where stdout must stay
/// pure diff/JSON (AGENTS.md), so its progress reports on stderr —
/// failures would already surface as per-file missing-tool errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]

pub(crate) enum ProgressTarget {
    Stdout,
    Stderr,
}

/// The shared install core, used by `u50 --setup` and by the engine's
/// batched provisioning pre-pass: initializes uv's preview state, drives
/// the async provisioning pipeline ([`provision_backends`]) on a local
/// runtime — one parallel wheel-fetch task per package, one serialized
/// venv install — reports the `installing N package(s)` banner and the
/// per-package summary lines (`installed:` / `failed:`) on
/// [`ProgressTarget`], and bails when anything failed.
///
/// A cross-process advisory lock (`<cache>/.provision.lock`) is held for
/// the whole run: venv creation, wheel extraction, and the venv install
/// all mutate shared cache state, and two concurrent cold-start
/// processes would otherwise race (torn extractions, half-created venvs).
///
/// # Errors
/// Returns an error when uv provisioning fails or any package failed to
/// install.
pub(crate) fn install_backends(missing: &[(String, String)], target: ProgressTarget) -> Result<()> {
    let report = |line: String| match target {
        ProgressTarget::Stdout => println!("{line}"),
        ProgressTarget::Stderr => eprintln!("{line}"),
    };

    // Several uv crates read the process-global preview state; initialize
    // it before touching any uv API.
    uv_preview::set(Preview::default()).context("preview init")?;

    // Stays synchronous (the CLI and the engine pre-pass call this
    // synchronously); the uv provisioning path is async, so drive it on a
    // local runtime.
    let runtime = Runtime::new().context("tokio runtime")?;
    let cache = cache_dir().context("resolve the u50 style cache directory")?;
    // The lock file lives inside the cache; a cold cache has no directory
    // yet (fslock will not create parents).
    std::fs::create_dir_all(&cache).context("create the u50 style cache directory")?;
    // Cross-process serialization for the whole provisioning run (held
    // until `lock` drops at function end).
    let _lock = fslock::LockFile::open(&cache.join(".provision.lock"))
        .and_then(|mut lock| lock.lock().map(|()| lock))
        .context("acquire the cache provisioning lock")?;
    report(format!(
        "installing {} package(s) into {}",
        missing.len(),
        cache.display()
    ));
    let outcomes = runtime.block_on(provision_backends(&cache, missing))?;

    let mut any_failure = false;
    for outcome in &outcomes {
        match &outcome.failure {
            None => report(format!("installed: {} ({})", outcome.package, outcome.tool)),
            Some(reason) => {
                any_failure = true;
                report(format!("failed: {}: {reason}", outcome.package));
            }
        }
    }

    if any_failure {
        bail!("one or more formatter backends failed to install")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::missing_backends;
    use super::pins::{
        PINNED_VERSIONS, Role, TRANSITIVE_DEPS, pip_spec, transitive_deps, wheel_specs,
    };
    use super::wheels::{WHEEL_RANK_REJECT, dist_info_dir, hex_encode, wheel_rank};

    #[test]
    fn all_backends_resolved_yields_nothing() {
        assert!(missing_backends(|_| true).is_empty());
    }

    #[test]
    fn no_backend_resolved_lists_all_packages_with_clang_format_deduped() {
        let missing = missing_backends(|_| false);
        assert_eq!(
            missing,
            vec![
                ("clang-format".to_owned(), "clang-format".to_owned()),
                ("autopep8".to_owned(), "autopep8".to_owned()),
                ("jsbeautifier".to_owned(), "js-beautify".to_owned()),
                ("djhtml".to_owned(), "djhtml".to_owned()),
                ("cssbeautifier".to_owned(), "css-beautify".to_owned()),
                ("sqlparse".to_owned(), "sqlformat".to_owned()),
            ],
            "first-seen language order; C/C++/Java dedup to one clang-format entry"
        );
        assert_eq!(missing.len(), 6);
    }

    #[test]
    fn partial_resolution_skips_only_resolved_packages_in_order() {
        // clang-format (C/Cpp/Java) and djhtml (HTML) present: the
        // remaining four packages survive, in first-seen order.
        let missing = missing_backends(|tool| tool == "clang-format" || tool == "djhtml");
        assert_eq!(
            missing,
            vec![
                ("autopep8".to_owned(), "autopep8".to_owned()),
                ("jsbeautifier".to_owned(), "js-beautify".to_owned()),
                ("cssbeautifier".to_owned(), "css-beautify".to_owned()),
                ("sqlparse".to_owned(), "sqlformat".to_owned()),
            ]
        );
    }

    #[test]
    fn every_pip_package_is_pinned_to_the_tool_versions_fixture() {
        // `tool-versions.txt` is the CI/doc source of truth; every package
        // reachable from Language::ALL must carry a matching pin.
        let txt = include_str!("../../tests/tool-versions.txt");
        for &language in &crate::language::Language::ALL {
            let pkg = language.pip_package();
            let (_, version) = PINNED_VERSIONS
                .iter()
                .find(|(p, _)| *p == pkg)
                .unwrap_or_else(|| panic!("{pkg} has no pin in PINNED_VERSIONS"));
            assert!(
                txt.lines().any(|l| *l == format!("{pkg}=={version}")),
                "pin {pkg}=={version} must appear verbatim in tests/tool-versions.txt"
            );
        }
    }

    #[test]
    fn pip_spec_pins_known_packages_and_passes_unknown_bare() {
        assert_eq!(pip_spec("autopep8"), "autopep8==2.3.2");
        assert_eq!(pip_spec("clang-format"), "clang-format==22.1.8");
        assert_eq!(pip_spec("not-a-pinned-package"), "not-a-pinned-package");
    }

    #[test]
    fn dist_info_dir_is_best_effort_for_malformed_stems() {
        assert_eq!(
            dist_info_dir("autopep8-2.3.2-py2.py3-none-any"),
            Some("autopep8-2.3.2.dist-info".to_owned())
        );
        assert_eq!(
            dist_info_dir(
                "clang_format-22.1.8-py2.py3-none-manylinux_2_27_x86_64.manylinux_2_28_x86_64"
            ),
            Some("clang_format-22.1.8.dist-info".to_owned())
        );
        // Malformed stems still yield a best-effort name; a wrong name
        // never matches a real dist-info dir, so reuse fails harmlessly.
        assert_eq!(
            dist_info_dir("just-a-name"),
            Some("just-a.dist-info".to_owned())
        );
        assert_eq!(dist_info_dir("nodashes"), None);
    }

    #[test]
    fn wheel_specs_add_transitive_deps_after_the_primary() {
        let missing = vec![("autopep8".to_owned(), "autopep8".to_owned())];
        assert_eq!(
            wheel_specs(&missing),
            vec![
                (
                    "autopep8".to_owned(),
                    Some("2.3.2".to_owned()),
                    Role::Primary
                ),
                // Dependencies are pinned too (PINNED_VERSIONS), so a
                // cold start cannot silently install a newer release.
                (
                    "pycodestyle".to_owned(),
                    Some("2.14.0".to_owned()),
                    Role::Dependency
                ),
            ]
        );
    }

    #[test]
    fn wheel_specs_dedup_shared_transitive_deps() {
        // `editorconfig` and `six` are deps of both jsbeautifier and
        // cssbeautifier: they must be fetched once, after their first
        // parent.
        let missing = vec![
            ("jsbeautifier".to_owned(), "js-beautify".to_owned()),
            ("cssbeautifier".to_owned(), "css-beautify".to_owned()),
        ];
        let specs = wheel_specs(&missing);
        let names: Vec<&str> = specs.iter().map(|(name, _, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec!["jsbeautifier", "editorconfig", "six", "cssbeautifier"]
        );
        assert!(
            specs
                .iter()
                .filter(|(name, _, role)| *role == Role::Dependency
                    && (name == "editorconfig" || name == "six"))
                .count()
                == 2,
            "each shared dep is listed exactly once"
        );
    }

    #[test]
    fn wheel_rank_prefers_pure_then_the_host_platform_and_rejects_the_rest() {
        let arch = std::env::consts::ARCH;
        let windows_host = std::env::consts::OS == "windows";
        let pure = "pkg-1.0-py3-none-any.whl".to_owned();
        let (host_platform, foreign) = if windows_host {
            let tag = if arch == "aarch64" {
                "win_arm64"
            } else {
                "win_amd64"
            };
            (
                format!("pkg-1.0-cp314-cp314-{tag}.whl"),
                format!("pkg-1.0-cp314-cp314-manylinux_2_17_{arch}.whl"),
            )
        } else {
            (
                format!("pkg-1.0-cp314-cp314-manylinux_2_17_{arch}.whl"),
                "pkg-1.0-cp314-cp314-win_amd64.whl".to_owned(),
            )
        };
        let linux = format!("pkg-1.0-cp314-cp314-linux_{arch}.whl");
        let musl = format!("pkg-1.0-cp314-cp314-musllinux_1_2_{arch}.whl");
        assert!(wheel_rank(&pure) > wheel_rank(&host_platform));
        assert!(wheel_rank(&host_platform) > WHEEL_RANK_REJECT);
        assert_eq!(wheel_rank(&foreign), WHEEL_RANK_REJECT);
        if windows_host {
            assert_eq!(wheel_rank(&linux), WHEEL_RANK_REJECT);
        } else {
            assert!(wheel_rank(&linux) > WHEEL_RANK_REJECT);
        }
        assert_eq!(wheel_rank(&musl), WHEEL_RANK_REJECT);
    }

    #[test]
    fn hex_encode_is_lowercase_hex_and_matches_known_digests() {
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(
            hex_encode(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn transitive_deps_table_is_consistent_with_the_pins() {
        for (parent, deps) in TRANSITIVE_DEPS {
            assert!(
                PINNED_VERSIONS.iter().any(|(p, _)| p == parent),
                "{parent} is a backend package and must be pinned"
            );
            for dep in *deps {
                assert!(
                    PINNED_VERSIONS.iter().any(|(p, _)| p == dep),
                    "{dep} is a transitive dependency and must be pinned \
                     (a dep-chain change would otherwise fail only at runtime)"
                );
                assert!(
                    transitive_deps(dep).is_empty(),
                    "{dep} must not itself declare transitive deps (no dep chains)"
                );
            }
        }
    }
}
