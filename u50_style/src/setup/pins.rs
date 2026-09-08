//! The pinned package versions and their transitive-dependency table.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// A missing backend package.
    Primary,
    /// A transitive dependency of a [`Role::Primary`] package.
    Dependency,
}

/// One backend package's pinned version, if pinned in
/// [`PINNED_VERSIONS`].
pub(crate) const PINNED_VERSIONS: &[(&str, &str)] = &[
    ("clang-format", "22.1.8"),
    ("autopep8", "2.3.2"),
    ("jsbeautifier", "2.0.3"),
    ("cssbeautifier", "2.0.3"),
    ("djhtml", "3.0.11"),
    ("sqlparse", "0.5.3"),
    ("pycodestyle", "2.14.0"),
    ("editorconfig", "0.17.1"),
    ("six", "1.17.0"),
];

pub(crate) const TRANSITIVE_DEPS: &[(&str, &[&str])] = &[
    ("autopep8", &["pycodestyle"]),
    ("jsbeautifier", &["editorconfig", "six"]),
    ("cssbeautifier", &["editorconfig", "six"]),
];

pub(crate) fn pinned_version(package: &str) -> Option<&'static str> {
    PINNED_VERSIONS
        .iter()
        .find(|(pkg, _)| *pkg == package)
        .map(|(_, version)| *version)
}

/// The pinned wheel spec for `package`: `<pkg>==<version>` when pinned
/// in [`PINNED_VERSIONS`], the bare package name otherwise. Test-only:
/// production resolves via [`pinned_version`] and the `PyPI` JSON API.
#[cfg(test)]
pub(crate) fn pip_spec(package: &str) -> String {
    pinned_version(package).map_or_else(
        || package.to_owned(),
        |version| format!("{package}=={version}"),
    )
}

/// The hardcoded transitive dependencies of `package` (empty when none
/// are declared in [`TRANSITIVE_DEPS`]); each one carries a pin in
/// [`PINNED_VERSIONS`].
pub(crate) fn transitive_deps(package: &str) -> &'static [&'static str] {
    TRANSITIVE_DEPS
        .iter()
        .find(|(pkg, _)| *pkg == package)
        .map_or(&[], |(_, deps)| *deps)
}

/// Builds the full wheel spec list for `missing`: every backend package
/// with its pinned version (or `None` to resolve latest via `PyPI`),
/// followed by its transitive dependencies, deduped by package name
/// (first occurrence wins, so a primary never gets downgraded to a
/// dependency by a later parent).
pub(crate) fn wheel_specs(missing: &[(String, String)]) -> Vec<(String, Option<String>, Role)> {
    let mut specs: Vec<(String, Option<String>, Role)> = Vec::new();
    for (package, _) in missing {
        if !specs.iter().any(|(name, _, _)| name == package) {
            specs.push((
                package.clone(),
                pinned_version(package).map(str::to_owned),
                Role::Primary,
            ));
        }
        for &dep in transitive_deps(package) {
            if !specs.iter().any(|(name, _, _)| name == dep) {
                // Dependencies are pinned too: an unpinned dep would
                // silently install whatever was latest on PyPI at
                // provision time, defeating the reproducibility the
                // primary pins exist for.
                specs.push((
                    dep.to_owned(),
                    pinned_version(dep).map(str::to_owned),
                    Role::Dependency,
                ));
            }
        }
    }
    specs
}
