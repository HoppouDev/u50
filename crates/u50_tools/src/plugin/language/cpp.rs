use super::LanguagePlugin;
use crate::plugin::formatter::clang_format;

inventory::submit! {
    LanguagePlugin {
        id: "cpp",
        display_name: "C++",
        extensions: &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        formatter: clang_format::ID,
    }
}

#[cfg(test)]
mod tests {
    use crate::plugin::language::test_support;

    #[test]
    fn registers_itself_in_the_language_registry() {
        test_support::assert_registered("cpp", "C++", "clang-format");
    }

    #[test]
    fn detects_cpp_files_by_extension() {
        test_support::assert_detects_all("cpp", &["cpp", "cc", "cxx", "hpp", "hh", "hxx"]);
    }

    #[test]
    fn does_not_detect_extensionless_files_as_cpp() {
        test_support::assert_does_not_detect("cpp", "README");
    }

    #[test]
    fn plain_c_extension_does_not_detect_as_cpp() {
        test_support::assert_does_not_detect("cpp", "main.c");
    }
}
