use super::LanguagePlugin;
use crate::plugin::formatter::clang_format;

inventory::submit! {
	LanguagePlugin {
		id: "java",
		display_name: "Java",
		extensions: &["java"],
		formatter: clang_format::ID,
	}
}

#[cfg(test)]
mod tests {
	use crate::plugin::language::test_support;

	#[test]
	fn registers_itself_in_the_language_registry() {
		test_support::assert_registered("java", "Java", "clang-format");
	}

	#[test]
	fn detects_java_files_by_extension() {
		test_support::assert_detects_all("java", &["java"]);
	}

	#[test]
	fn does_not_detect_compiled_class_files_as_java() {
		test_support::assert_does_not_detect("java", "Main.class");
	}

	#[test]
	fn does_not_detect_extensionless_files_as_java() {
		test_support::assert_does_not_detect("java", "README");
	}
}
