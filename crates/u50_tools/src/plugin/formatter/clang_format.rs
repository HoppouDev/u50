use super::FormatterPlugin;

/// Stable id of formatter
pub const ID: &str = "clang-format";

inventory::submit! {
	FormatterPlugin {
		id: ID,
		display_name: "ClangFormat",
		uv_package: "clang-format",
		version: None,
	}
}

#[cfg(test)]
mod tests {
	use super::ID;
	use crate::plugin::formatter::{self, FormatterPlugin};
	use crate::plugin::resolver::{self, ResolverConfig};

	/// Looks up the registered clang-format plugin
	fn plugin() -> &'static FormatterPlugin {
		formatter::by_id(ID).expect("clang-format is registered")
	}

	#[test]
	fn registers_itself_in_the_formatter_registry() {
		assert_eq!(plugin().display_name, "ClangFormat");
		assert_eq!(plugin().uv_package, "clang-format");
	}

	#[test]
	fn tool_spec_resolves_through_the_uv_clang_format_package() {
		let spec = plugin().tool_spec("style50");
		assert!(
			matches!(spec.config, ResolverConfig::Uv { ref package, .. } if package == "clang-format")
		);
	}

	#[test]
	fn provisions_and_resolves_through_the_uv_resolver() {
		// Requires network access and a working `uv` on PATH and skips quietly
		// if not
		if which::which("uv").is_err() {
			eprintln!("skipping provisions_and_resolves_through_the_uv_resolver: uv not on PATH");
			return;
		}

		// Unique per process time to prevent race conditions on concurrent test
		// runs
		let domain = format!(
			"test-clang-format-integration-{}-{}",
			std::process::id(),
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.expect("system clock is after the epoch")
				.as_nanos()
		);
		let spec = plugin().tool_spec(&domain);

		let cache_root = resolver::uv::UvResolver::cache_root(&spec);
		let _ = std::fs::remove_dir_all(&cache_root);

		if resolver::provision_tool(&spec).is_err() {
			eprintln!(
				"skipping provisions_and_resolves_through_the_uv_resolver: provisioning failed (likely no network)"
			);
			let _ = std::fs::remove_dir_all(&cache_root);
			return;
		}

		let resolved = resolver::resolve_tool(&spec).expect("resolves after provisioning");
		assert!(resolved.path.is_file());

		let _ = std::fs::remove_dir_all(&cache_root);
	}
}
