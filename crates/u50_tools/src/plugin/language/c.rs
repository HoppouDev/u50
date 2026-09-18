use super::LanguagePlugin;
use crate::plugin::formatter::clang_format;

inventory::submit! {
	LanguagePlugin {
		id: "c",
		display_name: "C",
		extensions: &["c", "h"],
		formatter: clang_format::ID,
	}
}

#[cfg(test)]
mod tests {
	use crate::plugin::language::test_support;

	#[test]
	fn registers_itself_in_the_language_registry() {
		test_support::assert_registered("c", "C", "clang-format");
	}

	#[test]
	fn detects_c_files_by_extension() {
		test_support::assert_detects_all("c", &["c", "h"]);
	}

	#[test]
	fn does_not_detect_extensionless_files_as_c() {
		test_support::assert_does_not_detect("c", "README");
	}
}
