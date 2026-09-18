use std::path::PathBuf;

use anyhow::Context;
use tracing::error;
use u50_tools::plugin::{formatter, resolver};
use walkdir::WalkDir;

use crate::format::style_one_file;

/// Runs the cs50 style checker against `paths`
pub fn style_paths(paths: &Vec<PathBuf>, write: bool) -> anyhow::Result<()> {
	for path in paths {
		// Continue if `path` does not exist
		if !path.exists() {
			error!("Path {:?} does not exist", path);
			continue;
		}

		// Use `walkdir::WalkDir` if `path` is a directory
		if path.is_dir() {
			for entry in WalkDir::new(path).follow_links(true).max_depth(100) {
				let entry =
					entry.with_context(|| format!("walking directory `{}`", path.display()))?;
				if entry.path().is_file() {
					style_one_file(entry.path(), write)?;
				}
			}
		} else if path.is_file() {
			style_one_file(path, write)?;
		}
	}

	Ok(())
}

/// Resolves the formatter plugin registered under `formatter_id`
pub(crate) fn resolve_formatter(
	formatter_id: &str,
) -> anyhow::Result<(&'static formatter::FormatterPlugin, resolver::Resolved)> {
	let formatter = formatter::by_id(formatter_id)
		.ok_or_else(|| anyhow::anyhow!("no `{formatter_id}` formatter plugin is registered"))?;

	let spec = formatter.tool_spec("style50");
	let resolved = match resolver::resolve_tool(&spec) {
		Some(resolved) => resolved,
		None => {
			resolver::provision_tool(&spec)?;
			resolver::resolve_tool(&spec).expect("just provisioned")
		}
	};

	Ok((formatter, resolved))
}
