use std::fmt;

use crossterm::style::Stylize;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

/// Formats log events
pub struct ArrowFormatter;

impl<S, N> FormatEvent<S, N> for ArrowFormatter
where
	S: Subscriber + for<'a> LookupSpan<'a>,
	N: for<'a> FormatFields<'a> + 'static,
{
	fn format_event(
		&self,
		ctx: &FmtContext<'_, S, N>,
		mut writer: Writer<'_>,
		event: &Event<'_>,
	) -> fmt::Result {
		let level = *event.metadata().level();
		let level = match level {
			Level::TRACE => "TRACE".magenta().bold().to_string(),
			Level::DEBUG => "DEBUG".blue().bold().to_string(),
			Level::INFO => " INFO".green().bold().to_string(),
			Level::WARN => " WARN".yellow().bold().to_string(),
			Level::ERROR => "ERROR".red().bold().to_string(),
		};

		write!(writer, "{level} {} ", "»".dark_grey())?;

		ctx.field_format().format_fields(writer.by_ref(), event)?;

		writeln!(writer)
	}
}
