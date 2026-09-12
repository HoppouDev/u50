//! Shared filesystem plumbing: symlink-safe recursive copy.

use std::path::Path;

use anyhow::Context as _;

/// Copies `src` to `dst`, recursively when `src` is a directory.
/// Never follows symlinks from student code: a cycle would recurse
/// forever, and a link could point anywhere on the host filesystem.
///
/// # Errors
/// Returns an error when any copy step fails.
pub fn copy_tree(src: &Path, dst: &Path) -> anyhow::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)
            .with_context(|| format!("could not create {}", dst.display()))?;
        for entry in
            std::fs::read_dir(src).with_context(|| format!("could not read {}", src.display()))?
        {
            let entry = entry.context("read dir entry")?;
            if entry.file_type().is_ok_and(|ft| ft.is_symlink()) {
                tracing::warn!(path = %entry.path().display(), "skipping symlink");
                continue;
            }
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)
            .with_context(|| format!("could not copy {} to {}", src.display(), dst.display()))?;
        Ok(())
    }
}
