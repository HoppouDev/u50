use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};

/// Color scheme for help and error output
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::BrightBlue.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::BrightBlue.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(clap::Parser)]
#[command(
    name = "u50",
    version,
    about,
    long_about = None,
    propagate_version = true,
    styles = STYLES
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Submit a problem
    Submit {},

    /// Check a program's correctness
    Check {},

    /// Check the code's style against CS50's style guide
    Style {},
}
