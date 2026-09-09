//! C/C++/Java: the C-family string stripping, the clang-format backend
//! shared by the three plugins, and the three plugin structs (one
//! family, three registrations — the registry entry points live in
//! `crate::registry`).

use super::LanguagePlugin;
use super::count_c_family_comments;
use crate::format::run_tool;

/// The clang-format style configuration CS50 uses for its style checks
/// (recorded verbatim from the original `style50` source).
const CS50_CLANG_FORMAT_CONFIG: &str = "{ \
AllowShortFunctionsOnASingleLine: Empty, \
BraceWrapping: { AfterCaseLabel: true, AfterControlStatement: true, \
AfterFunction: true, AfterStruct: true, BeforeElse: true, BeforeWhile: true }, \
BreakBeforeBraces: Custom, ColumnLimit: 100, IndentCaseLabels: true, \
IndentWidth: 4, SpaceAfterCStyleCast: true, TabWidth: 4 }";

/// Formats C-family source with `clang-format`, passing the canonical
/// file name (`--assume-filename`) so the right lexer is picked.
///
/// # Errors
/// Returns an error when `clang-format` is missing or fails.
fn format_clang(source: &str, file_name: &str) -> anyhow::Result<String> {
    let assume = format!("--assume-filename={file_name}");
    let style = format!("-style={CS50_CLANG_FORMAT_CONFIG}");
    run_tool("clang-format", &[assume.as_str(), style.as_str()], source)
}

/// The C language plugin.
pub(crate) struct CPlugin;
pub(crate) static C_PLUGIN: CPlugin = CPlugin;

impl LanguagePlugin for CPlugin {
    fn id(&self) -> &'static str {
        "c"
    }

    fn display_name(&self) -> &'static str {
        "C"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["c", "h"]
    }

    fn required_tool(&self) -> &'static str {
        "clang-format"
    }

    fn pip_package(&self) -> Option<&'static str> {
        Some("clang-format")
    }

    fn assume_filename(&self) -> Option<&'static str> {
        Some("foo.c")
    }

    fn count_comments(&self, code: &str) -> Option<u32> {
        Some(count_c_family_comments(code))
    }

    fn missing_tool_message(&self) -> String {
        "clang-format is required (>= 14) to check C/C++/Java style".to_owned()
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        let file_name = self
            .assume_filename()
            .unwrap_or_else(|| unreachable!("C-family plugins always declare an assume-filename"));
        format_clang(source, file_name)
    }
}

/// The C++ language plugin (shares C's backend and counter).
pub(crate) struct CppPlugin;
pub(crate) static CPP_PLUGIN: CppPlugin = CppPlugin;

impl LanguagePlugin for CppPlugin {
    fn id(&self) -> &'static str {
        "cpp"
    }

    fn display_name(&self) -> &'static str {
        "C++"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["cpp", "hpp"]
    }

    fn required_tool(&self) -> &'static str {
        "clang-format"
    }

    fn pip_package(&self) -> Option<&'static str> {
        Some("clang-format")
    }

    fn assume_filename(&self) -> Option<&'static str> {
        Some("foo.cpp")
    }

    fn count_comments(&self, code: &str) -> Option<u32> {
        Some(count_c_family_comments(code))
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        let file_name = self
            .assume_filename()
            .unwrap_or_else(|| unreachable!("C-family plugins always declare an assume-filename"));
        format_clang(source, file_name)
    }
}

/// The Java language plugin (shares C's backend and counter).
pub(crate) struct JavaPlugin;
pub(crate) static JAVA_PLUGIN: JavaPlugin = JavaPlugin;

impl LanguagePlugin for JavaPlugin {
    fn id(&self) -> &'static str {
        "java"
    }

    fn display_name(&self) -> &'static str {
        "Java"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["java"]
    }

    fn required_tool(&self) -> &'static str {
        "clang-format"
    }

    fn pip_package(&self) -> Option<&'static str> {
        Some("clang-format")
    }

    fn assume_filename(&self) -> Option<&'static str> {
        Some("foo.java")
    }

    fn count_comments(&self, code: &str) -> Option<u32> {
        Some(count_c_family_comments(code))
    }

    fn format(&self, source: &str) -> anyhow::Result<String> {
        let file_name = self
            .assume_filename()
            .unwrap_or_else(|| unreachable!("C-family plugins always declare an assume-filename"));
        format_clang(source, file_name)
    }
}
