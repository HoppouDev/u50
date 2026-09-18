use std::path::Path;

use anyhow::Context;

use crossterm::style::Stylize;

use crate::{format::format_file, util::get_terminal_width};

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

const GUTTER_WIDTH: usize = 9;

/// Terminal columns available for content after the gutter
fn content_width() -> usize {
	get_terminal_width().saturating_sub(GUTTER_WIDTH).max(10)
}

/// Truncates `text` to at most `max_width` display columns appending an
/// ellipsis when it's cut short
pub(crate) fn truncate_to_width(text: &str, max_width: usize) -> String {
	use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

	if text.width() <= max_width {
		return text.to_owned();
	}
	if max_width == 0 {
		return String::new();
	}

	let budget = max_width.saturating_sub(1);
	let mut width = 0;
	let mut truncated = String::new();
	for ch in text.chars() {
		let ch_width = ch.width().unwrap_or(0);
		if width + ch_width > budget {
			break;
		}
		width += ch_width;
		truncated.push(ch);
	}
	truncated.push('…');
	truncated
}

/// Colors a style50-style line diff. The line-number gutter (`nnnn │`)
/// always stays neutral/dark grey, matching the box border; only the
/// marker + content that follows it takes on red/green/grey
pub(crate) fn colorize_diff(diff: &str) -> String {
	let mut old_line = 0usize;
	let mut new_line = 0usize;
	let max_width = content_width();

	diff.lines()
		.map(|line| {
			let rendered = if let Some(rest) = line.strip_prefix("- ") {
				old_line += 1;
				let gutter = format!("{} │", format_line_number(old_line))
					.dark_grey()
					.to_string();
				let rest = truncate_to_width(rest, max_width);
				let body = format!(" - {rest}").red().bold().to_string();
				format!("{gutter}{body}")
			} else if let Some(rest) = line.strip_prefix("+ ") {
				new_line += 1;
				let gutter = format!("{} │", format_line_number(new_line))
					.dark_grey()
					.to_string();
				let rest = truncate_to_width(rest, max_width);
				let body = format!(" + {rest}").green().bold().to_string();
				format!("{gutter}{body}")
			} else {
				old_line += 1;
				new_line += 1;
				let gutter = format!("{} │", format_line_number(new_line))
					.dark_grey()
					.to_string();
				let content = line.strip_prefix("  ").unwrap_or(line);
				let content = truncate_to_width(content, max_width);
				let body = format!(" ~ {content}").dark_grey().bold().to_string();
				format!("{gutter}{body}")
			};
			rendered + "\n"
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
