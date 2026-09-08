//! The style50 ANSI palette, emitted through crossterm: every escape the
//! renderers produce comes from a crossterm command's ANSI writer (see
//! [`pinned`]), never from a hand-rolled byte string. Colored output is
//! visually identical to the original's termcolor rendering; the bytes
//! follow crossterm's SGR emission (8-bit `38;5;N`/`48;5;N` colors) rather
//! than termcolor's 3-bit `30-37`/`40-47` codes.

use std::sync::LazyLock;

use crossterm::Command;
use crossterm::style::{Attribute, Color, SetAttribute, SetBackgroundColor, SetForegroundColor};

/// Renders a crossterm command to its ANSI form once and pins it for the
/// process lifetime: the style50 palette is a fixed set of sequences, so
/// the pinned strings are bounded, and handing back `&'static str` keeps
/// the character-mode hot path allocation-free.
fn pinned(command: impl Command) -> &'static str {
    let mut buf = String::new();
    command
        .write_ansi(&mut buf)
        .expect("writing to a String cannot fail");
    Box::leak(buf.into_boxed_str())
}

/// Declares one palette accessor whose sequence is emitted through
/// crossterm's ANSI writer ([`pinned`]) on first use.
macro_rules! palette {
    ($($(#[$doc:meta])* $name:ident = $command:expr;)*) => {
        $(
            $(#[$doc])*
            #[must_use]
            pub(crate) fn $name() -> &'static str {
                static SEQUENCE: LazyLock<&'static str> = LazyLock::new(|| pinned($command));
                *SEQUENCE
            }
        )*
    };
}

palette! {
    /// Red foreground — termcolor's "red" (SGR 31): split-mode deletion
    /// columns.
    red = SetForegroundColor(Color::DarkRed);
    /// Green foreground — termcolor's "green" (SGR 32): split-mode
    /// insertion columns and character mode's `Looks good!`.
    green = SetForegroundColor(Color::DarkGreen);
    /// Yellow foreground — termcolor's "yellow" (SGR 33): the comments
    /// hints and score-mode error lines.
    yellow = SetForegroundColor(Color::DarkYellow);
    /// Cyan foreground — termcolor's "cyan" (SGR 36): the per-file
    /// `::::::::::::::` header of character mode.
    cyan = SetForegroundColor(Color::DarkCyan);
    /// Bright white foreground — termcolor's "white" (SGR 97): the
    /// character-mode banner (which also carries [`bold`]).
    bright_white = SetForegroundColor(Color::White);
    /// Green background — termcolor's "on green" (SGR 42):
    /// character-mode insertions.
    on_green = SetBackgroundColor(Color::DarkGreen);
    /// Red background — termcolor's "on red" (SGR 41): character-mode
    /// deletions.
    on_red = SetBackgroundColor(Color::DarkRed);
    /// Reset all attributes (SGR 0).
    reset = SetAttribute(Attribute::Reset);
    /// Bold (SGR 1).
    bold = SetAttribute(Attribute::Bold);
}
