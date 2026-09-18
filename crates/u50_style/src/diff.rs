use std::path::Path;

use anyhow::Context;

use crossterm::style::Stylize;

use crate::format::format_file;

/// Pads `line_number` to 4 digits with leading spaces; if it's wider than
/// 4 digits, the leftmost (most significant) digits are dropped instead of
/// letting the column grow
fn format_line_number(line_number: usize) -> String {
	let digits = line_number.to_string();
	let digits = if digits.len() > 4 {
		&digits[digits.len() - 4..]
	} else {
		&digits
	};
	format!("{digits:>4}")
}

/// Colors a style50-style line diff
pub(crate) fn colorize_diff(diff: &str) -> String {
	diff.lines()
		.enumerate()
		.map(|(line_number, line)| {
			let line_number = format_line_number(line_number + 1);

			let colored = if let Some(rest) = line.strip_prefix("- ") {
				format!("{line_number} - {rest}").red().bold().to_string()
			} else if let Some(rest) = line.strip_prefix("+ ") {
				format!("{line_number} + {rest}").green().bold().to_string()
			} else {
				format!("{line_number} ~ {}", line)
					.dark_grey()
					.bold()
					.to_string()
			};
			colored + "\n"
		})
		.collect()
}

/// Diffs `path` against its formatted version
pub(crate) fn diff_file_unified(path: &Path) -> anyhow::Result<String> {
	let original =
		std::fs::read_to_string(path).with_context(|| format!("reading `{}`", path.display()))?;
	let formatted = format_file(path)?;

	if original == formatted {
		return Ok(String::new());
	}

	let diff = similar::TextDiff::from_lines(&original, &formatted);
	let mut rendered = String::new();
	for change in diff.iter_all_changes() {
		let prefix = match change.tag() {
			similar::ChangeTag::Delete => "- ",
			similar::ChangeTag::Insert => "+ ",
			similar::ChangeTag::Equal => "  ",
		};
		rendered.push_str(prefix);
		rendered.push_str(change.value().trim_end_matches('\n'));
		rendered.push('\n');
	}

	Ok(rendered)
}
