use std::{
	io::Write,
	path::Path,
	process::{Command, Stdio},
};

use anyhow::Context;
use crossterm::style::Stylize;
use tracing::debug;
use u50_tools::plugin::*;

use crate::diff::{colorize_diff, diff_file_unified};
use crate::util::resolve_formatter;

const CLANG_FORMAT_STYLE: &str = "-style={ AllowShortFunctionsOnASingleLine: Empty, BraceWrapping: { AfterCaseLabel: true, AfterControlStatement: true, AfterFunction: true, AfterStruct: true, BeforeElse: true, BeforeWhile: true }, BreakBeforeBraces: Custom, ColumnLimit: 100, IndentCaseLabels: true, IndentWidth: 4, SpaceAfterCStyleCast: true, TabWidth: 4 }";

/// Formats `path` with its detected language's formatter
pub fn format_file(path: &Path) -> anyhow::Result<String> {
	debug!("Applying formatting to file {:?}", path.to_string_lossy());

	let plugin = language::detect(path)
		.ok_or_else(|| anyhow::anyhow!("no language plugin detected for `{}`", path.display()))?;

	let (formatter, resolved) = resolve_formatter(plugin.formatter)?;

	let output = Command::new(&resolved.path)
		.arg(path)
		.arg(CLANG_FORMAT_STYLE)
		.output()
		.with_context(|| {
			format!(
				"running `{}` on `{}`",
				resolved.path.display(),
				path.display()
			)
		})?;

	if !output.status.success() {
		anyhow::bail!(
			"{} exited with an error formatting `{}`: {}",
			formatter.display_name,
			path.display(),
			String::from_utf8_lossy(&output.stderr).trim()
		);
	}

	String::from_utf8(output.stdout).with_context(|| {
		format!(
			"`{}`'s output for `{}` was not valid UTF-8",
			formatter.display_name,
			path.display()
		)
	})
}

/// Formats one file and rewrites it in place when `write` is set
pub(crate) fn style_one_file(path: &Path, write: bool) -> anyhow::Result<()> {
	if write {
		let formatted = format_file(path)?;
		std::fs::write(path, &formatted)
			.with_context(|| format!("writing formatted output back to `{}`", path.display()))?;
		println!("{} {}", "formatted".green(), path.display());
	} else {
		let diff = diff_file_unified(path)?;
		if diff.is_empty() {
			println!("{} {}", "unchanged".dark_grey(), path.display());
		} else {
			println!(" {} ", path.to_string_lossy().on_blue());

			print!("{}", colorize_diff(&diff));
		}
	}

	Ok(())
}

/// Formats `source` in memory using `language_id`'s registered formatter
pub fn format_string(source: &str, language_id: &str) -> anyhow::Result<String> {
	let plugin = language::by_id(language_id)
		.ok_or_else(|| anyhow::anyhow!("no language plugin registered for `{language_id}`"))?;

	let extension = plugin
		.extensions
		.first()
		.ok_or_else(|| anyhow::anyhow!("language `{language_id}` declares no file extensions"))?;

	let (formatter, resolved) = resolve_formatter(plugin.formatter)?;

	let mut child = Command::new(&resolved.path)
		.arg(CLANG_FORMAT_STYLE)
		.arg(format!("--assume-filename=stdin.{extension}"))
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.with_context(|| format!("spawning `{}`", resolved.path.display()))?;

	child
		.stdin
		.take()
		.expect("stdin was requested as piped")
		.write_all(source.as_bytes())
		.with_context(|| format!("writing source to `{}`'s stdin", resolved.path.display()))?;

	let output = child
		.wait_with_output()
		.with_context(|| format!("waiting for `{}` to finish", resolved.path.display()))?;

	if !output.status.success() {
		anyhow::bail!(
			"{} exited with an error formatting the given string: {}",
			formatter.display_name,
			String::from_utf8_lossy(&output.stderr).trim()
		);
	}

	String::from_utf8(output.stdout)
		.with_context(|| format!("`{}`'s output was not valid UTF-8", formatter.display_name))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_string_reformats_messy_c_source() {
		if which::which("uv").is_err() {
			eprintln!("skipping format_string_reformats_messy_c_source: uv not on PATH");
			return;
		}

		let source = "int main( ) {\nint x=1;return x;}\n";
		let formatted = match format_string(source, "c") {
			Ok(formatted) => formatted,
			Err(err) => {
				eprintln!(
					"skipping format_string_reformats_messy_c_source: {err} (likely no network)"
				);
				return;
			}
		};

		assert_eq!(
			formatted,
			"int main()\n{\n    int x = 1;\n    return x;\n}\n"
		);
	}

	#[test]
	fn format_string_reformats_messy_java_source_without_assuming_c() {
		if which::which("uv").is_err() {
			eprintln!(
				"skipping format_string_reformats_messy_java_source_without_assuming_c: uv not on PATH"
			);
			return;
		}

		let source = "class Main{public static void main(String[] args){int x=1;}}\n";
		let formatted = match format_string(source, "java") {
			Ok(formatted) => formatted,
			Err(err) => {
				eprintln!(
					"skipping format_string_reformats_messy_java_source_without_assuming_c: {err} (likely no network)"
				);
				return;
			}
		};

		assert!(formatted.contains("class Main"));
		assert_ne!(formatted, source);
	}

	#[test]
	fn format_string_rejects_an_unknown_language_id() {
		assert!(format_string("int x;", "not-a-real-language").is_err());
	}
}
