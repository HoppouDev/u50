//! `--setup` support: installs missing formatter backends into u50's
//! cache (`~/.cache/u50/style50`) by downloading pip wheels in parallel
//! and installing them locally, so future runs can resolve the tools
//! from the cache without root or a system-wide install.

use std::process::Command;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::formatter::{ToolOrigin, cache_dir, locate_tool};
use crate::language::Language;

/// One `(pip package, backing tool)` pair that needs installing.
struct Missing {
    pip_package: String,
    tool: &'static str,
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
/// # Errors
/// Returns an error (CLI exit code 3) when pip is unavailable or any
/// package failed to install.
#[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
pub fn setup_missing() -> anyhow::Result<()> {
    // Distinct missing pip packages, in first-seen language order.
    let mut missing: Vec<Missing> = Vec::new();
    for &language in &Language::ALL {
        let Some(tool) = language.required_tool() else {
            continue;
        };
        if locate_tool(tool).is_some() {
            continue;
        }
        let pip_package = language.pip_package().to_owned();
        if !missing.iter().any(|m| m.pip_package == pip_package) {
            missing.push(Missing { pip_package, tool });
        }
    }

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
        for m in &missing {
            eprintln!(
                "failed: {}: python3/pip is not available (required to download {})",
                m.pip_package, m.tool
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
        .map(|m| {
            let pb = multi.add(ProgressBar::new_spinner());
            pb.set_style(style.clone());
            pb.set_message(format!("{} (for {})", m.pip_package, m.tool));
            let pip_package = m.pip_package.clone();
            let dest = wheels.clone();
            let pb = pb.clone();
            std::thread::spawn(move || {
                let output = Command::new("python3")
                    .args(["-m", "pip", "download", "--dest"])
                    .arg(&dest)
                    .arg(&pip_package)
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
    let downloaded: Vec<String> = results
        .iter()
        .filter(|(_, ok, _)| *ok)
        .map(|(pkg, _, _)| pkg.clone())
        .collect();
    let mut any_failure = false;
    if !downloaded.is_empty() {
        let target = cache.join("python");
        let install_ok = Command::new("python3")
            .args(["-m", "pip", "install", "--no-index", "--find-links"])
            .arg(&wheels)
            .arg("--target")
            .arg(&target)
            .args(&downloaded)
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
                .find(|m| &m.pip_package == pkg)
                .map_or("?", |m| m.tool);
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
