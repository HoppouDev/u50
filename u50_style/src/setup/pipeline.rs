//! The async provisioning pipeline: uv cache, venv, parallel wheel
//! fetches, one serialized install, and per-backend verification.

use std::path::Path;

use anyhow::{Context, Result};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use uv_cache::Cache;
use uv_client::BaseClientBuilder;
use uv_distribution_types::CachedDist;
use uv_installer::Installer;
use uv_preview::Preview;
use uv_python::PythonEnvironment;

use super::BackendOutcome;
use super::pins::{Role, transitive_deps, wheel_specs};
use super::venv::ensure_venv;
use super::wheels::fetch_wheel;
use crate::formatter::{ToolOrigin, locate_tool};

pub(crate) async fn provision_backends(
    cache_root: &Path,
    missing: &[(String, String)],
) -> Result<Vec<BackendOutcome>> {
    // One client builder for the whole run. `BaseClientBuilder::build()`
    // returns a middleware-wrapped client that already applies uv's
    // retry policy (3 retries, exponential backoff), a 10s connect
    // timeout, a 30s per-request read timeout, and a uv user agent.
    let client_builder = BaseClientBuilder::default();
    // The retry-enabled client backs the PyPI JSON and wheel GETs.
    let client = client_builder.build().context("http client build")?;
    // Python distribution downloads retry internally (`fetch_with_retry`),
    // so their client disables middleware retries to avoid double-retrying
    // â€” mirroring uv's own `installation.rs`.
    let download_client = client_builder
        .clone()
        .retries(0)
        .build()
        .context("download client build")?;
    // Persistent uv cache (NOT a temp dir): wheel archives persist across
    // runs, and the installer rejects symlink link-mode against temp
    // caches.
    let uv_cache = Cache::from_path(cache_root.join("uv"))
        .init()
        .await
        .context("uv cache init")?;
    let venv = ensure_venv(cache_root, &uv_cache, &client_builder, &download_client).await?;

    let specs = wheel_specs(missing);

    // Parallel fetches: one spawned task (and one spinner) per package.
    let multi = MultiProgress::new();
    let style = ProgressStyle::with_template("{spinner:.green} {msg}")
        .expect("static spinner template")
        .tick_chars("â ‹â ™â ¹â ¸â ¼â ´â ¦â §â ‡â  ");
    let wheels_dir = cache_root.join("wheels");
    let handles: Vec<_> = specs
        .clone()
        .into_iter()
        .map(|(name, version, role)| {
            let pb = multi.add(ProgressBar::new_spinner());
            pb.set_style(style.clone());
            pb.set_message(format!("fetching {name}"));
            let wheels_dir = wheels_dir.clone();
            let client = client.clone();
            let pb = pb.clone();
            tokio::spawn(async move {
                let result = fetch_wheel(&client, &name, version.as_deref(), &wheels_dir).await;
                match &result {
                    Ok(_) => pb.finish_with_message(format!("{name}: done")),
                    Err(_) => pb.finish_with_message(format!("{name}: FAILED")),
                }
                (name, role, result.map_err(|e| format!("{e:#}")))
            })
        })
        .collect();
    let mut fetches: Vec<(String, Role, Result<CachedDist, String>)> = Vec::new();
    for (handle, (name, _, role)) in handles.into_iter().zip(&specs) {
        fetches.push(match handle.await {
            Ok(fetch) => fetch,
            Err(e) => (name.clone(), *role, Err(format!("fetch task failed: {e}"))),
        });
    }

    // Install every successfully fetched wheel into the venv in one
    // blocking uv call; an install failure fails all backends.
    let dists: Vec<CachedDist> = fetches
        .iter()
        .filter_map(|(_, _, result)| result.as_ref().ok().cloned())
        .collect();
    let install_error = if dists.is_empty() {
        None
    } else {
        install_dists(venv, uv_cache, dists).await
    };

    // Per-backend verification: a backend counts as installed when its
    // wheel and all its transitive deps were fetched and installed and
    // its tool resolves from the cache bin dir afterwards.
    let mut outcomes = Vec::new();
    for (package, tool) in missing {
        let mut failure = fetches
            .iter()
            .find(|(name, role, _)| name == package && *role == Role::Primary)
            .and_then(|(_, _, result)| result.as_ref().err().cloned());
        if failure.is_none() {
            for dep in transitive_deps(package) {
                let dep_failure = fetches
                    .iter()
                    .find(|(name, _, _)| name == dep)
                    .and_then(|(_, _, result)| result.as_ref().err().cloned());
                if let Some(reason) = dep_failure {
                    failure = Some(format!("{dep}: {reason}"));
                    break;
                }
            }
        }
        if failure.is_none() {
            failure.clone_from(&install_error);
        }
        if failure.is_none()
            && !locate_tool(tool).is_some_and(|(_, origin)| origin == ToolOrigin::Cache)
        {
            failure = Some(format!(
                "tool `{tool}` still not found in the cache after install"
            ));
        }
        outcomes.push(BackendOutcome {
            package: package.clone(),
            tool: tool.clone(),
            failure,
        });
    }
    Ok(outcomes)
}

/// Installs every successfully fetched wheel into the venv in one
/// blocking uv call. An install failure (or blocking-task panic) fails
/// all backends; the returned string is the formatted install error.
///
/// `install_blocking` runs rayon's wheel-install pool on the calling
/// thread; keep that blocking work off the async runtime by moving the
/// venv and uv cache into the blocking task and building the
/// `Installer` inside it (its borrows are not `'static`, and neither
/// value is used past the install step).
async fn install_dists(
    venv: PythonEnvironment,
    uv_cache: Cache,
    dists: Vec<CachedDist>,
) -> Option<String> {
    match tokio::task::spawn_blocking(move || {
        Installer::new(&venv, Preview::default())
            .with_cache(&uv_cache)
            .with_installer_metadata(false)
            .install_blocking(dists)
            .err()
            .map(|e| format!("install: {e:#}"))
    })
    .await
    {
        Ok(inner) => inner,
        Err(e) => Some(format!("install: task failed: {e}")),
    }
}
