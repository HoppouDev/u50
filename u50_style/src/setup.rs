//! `--setup` support: installs missing formatter backends into u50's
//! cache (`~/.cache/u50/style50`) by downloading pip wheels in parallel
//! and installing them locally, so future runs can resolve the tools
//! from the cache without root or a system-wide install.

use std::process::Command;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::formatter::{ToolOrigin, cache_dir, locate_tool};
use crate::language::Language;

/// Pip package versions pinned for `--setup` downloads.
///
/// These MUST match `tests/tool-versions.txt` — the CI/doc source of truth
/// used to generate the golden fixtures (`tests/fixtures/`) and verified
/// byte-identical. Bumping a version here requires updating that file and
/// regenerating the golden fixtures together. Pinning `pip download` keeps
/// `--setup` installs byte-compatible with the fixtures.
const PINNED_VERSIONS: &[(&str, &str)] = &[
    ("clang-format", "22.1.8"),
    ("autopep8", "2.3.2"),
    ("jsbeautifier", "2.0.3"),
    ("cssbeautifier", "2.0.3"),
    ("djhtml", "3.0.6"),
    ("sqlparse", "0.5.3"),
];

/// The `pip download` spec for `package`: `<pkg>==<version>` when pinned
/// in [`PINNED_VERSIONS`], the bare package name otherwise.
fn pip_spec(package: &str) -> String {
    PINNED_VERSIONS
        .iter()
        .find(|(pkg, _)| *pkg == package)
        .map_or_else(
            || package.to_owned(),
            |(_, version)| format!("{package}=={version}"),
        )
}

/// Computes the distinct missing pip packages, in first-seen language
/// order: iterates [`Language::ALL`], skips languages whose backing tool
/// `is_resolved`, and dedups by pip package (C/C++/Java all share
/// `clang-format`, so they collapse to one entry). Pure decision logic so
/// the missing-backend computation is unit-testable without any
/// subprocess; the pip download/install subprocesses themselves are
/// exercised by manual smoke runs and the CI golden step, not by unit
/// tests (they need network access).
fn missing_backends(is_resolved: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut missing: Vec<(String, String)> = Vec::new();
    for &language in &Language::ALL {
        let Some(tool) = language.required_tool() else {
            continue;
        };
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

/// Installs missing formatter backends into the cache. Missing tools are
/// detected per language; their distinct pip packages are downloaded in
/// parallel (one thread per package, spinner per download) into
/// `<cache>/wheels` and then installed with one local
/// `pip install --no-index --find-links --target <cache>/python` call.
///
/// A package counts as installed only when its backing tool is resolvable
/// from the cache bin dir afterwards. Per-package summary lines are
/// printed (`installed: <pkg> (<tool>)` / `failed: <pkg>: <reason>`).
///
/// The missing-backend decision path is unit-tested in
/// [`missing_backends`]; the pip subprocess paths are covered by manual
/// smoke runs and the CI golden step (network), not unit tests.
///
/// # Errors
/// Returns an error (CLI exit code 3) when pip is unavailable or any
/// package failed to install.
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
pub fn setup_missing() -> anyhow::Result<()> {
    // Distinct missing pip packages, in first-seen language order.
    let missing = missing_backends(|tool| locate_tool(tool).is_some());

    let cache = cache_dir();
    if missing.is_empty() {
        println!("all formatter backends are already available");
        return Ok(());
    }
    println!(
        "installing {} package(s) into {}",
        missing.len(),
        cache.display()
    );

    // pip must exist before anything can be downloaded.
    let pip_ok = Command::new("python3")
        .args(["-m", "pip", "--version"])
        .output()
        .is_ok_and(|out| out.status.success());
    if !pip_ok {
        for (pip_package, tool) in &missing {
            eprintln!(
                "failed: {pip_package}: python3/pip is not available (required to download {tool})"
            );
        }
        anyhow::bail!("python3 with pip is required to install formatter backends");
    }

    // Parallel downloads: one thread (and one spinner) per package.
    let wheels = cache.join("wheels");
    let multi = MultiProgress::new();
    let style = ProgressStyle::with_template("{spinner:.green} {msg}")
        .expect("static spinner template")
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ");
    let handles: Vec<_> = missing
        .iter()
        .map(|(pip_package, tool)| {
            let pb = multi.add(ProgressBar::new_spinner());
            pb.set_style(style.clone());
            pb.set_message(format!("{pip_package} (for {tool})"));
            let pip_package = pip_package.clone();
            let dest = wheels.clone();
            let pb = pb.clone();
            std::thread::spawn(move || {
                let output = Command::new("python3")
                    .args(["-m", "pip", "download", "--dest"])
                    .arg(&dest)
                    // Pinned spec: same version the golden fixtures were
                    // generated with (see `tests/tool-versions.txt`).
                    .arg(pip_spec(&pip_package))
                    .output();
                let (ok, reason) = match output {
                    Ok(out) if out.status.success() => (true, String::new()),
                    Ok(out) => (
                        false,
                        String::from_utf8_lossy(&out.stderr).trim().to_owned(),
                    ),
                    Err(e) => (false, e.to_string()),
                };
                if ok {
                    pb.finish_with_message(format!("{pip_package}: done"));
                } else {
                    pb.finish_with_message(format!("{pip_package}: FAILED"));
                }
                (pip_package, ok, reason)
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("download thread panicked"))
        .collect();

    // Local install of the successfully downloaded wheels: one pip call.
    // Packages are passed as pinned specs (`<pkg>==<version>`), not bare
    // names: the `wheels` dir persists across runs, and without the pin a
    // stale higher-version wheel left there would win over the pinned one.
    let downloaded: Vec<String> = results
        .iter()
        .filter(|(_, ok, _)| *ok)
        .map(|(pkg, _, _)| pkg.clone())
        .collect();
    let mut any_failure = false;
    if !downloaded.is_empty() {
        let target = cache.join("python");
        let specs: Vec<String> = downloaded.iter().map(|pkg| pip_spec(pkg)).collect();
        let install_ok = Command::new("python3")
            .args(["-m", "pip", "install", "--no-index", "--find-links"])
            .arg(&wheels)
            .arg("--target")
            .arg(&target)
            .args(&specs)
            .output()
            .is_ok_and(|out| out.status.success());
        if !install_ok {
            eprintln!(
                "warning: pip install into {} failed; downloaded wheels remain in {}",
                target.display(),
                wheels.display()
            );
        }
        for pkg in &downloaded {
            let tool = missing
                .iter()
                .find(|(p, _)| p == pkg)
                .map_or("?", |(_, tool)| tool);
            if install_ok
                && locate_tool(tool).is_some_and(|(_, origin)| origin == ToolOrigin::Cache)
            {
                println!("installed: {pkg} ({tool})");
            } else {
                any_failure = true;
                println!("failed: {pkg}: tool `{tool}` still not found in the cache after install");
            }
        }
    }
    for (pkg, ok, reason) in &results {
        if *ok {
            continue;
        }
        any_failure = true;
        println!("failed: {pkg}: {reason}");
    }

    if any_failure {
        anyhow::bail!("one or more formatter backends failed to install")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{missing_backends, pip_spec};

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
        let txt = include_str!("../tests/tool-versions.txt");
        for &language in &crate::language::Language::ALL {
            let pkg = language.pip_package();
            let (_, version) = super::PINNED_VERSIONS
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
}
