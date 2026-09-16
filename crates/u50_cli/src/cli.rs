use std::path::PathBuf;

use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};
use clap::{Parser, Subcommand, ValueEnum};

/// Color scheme for help and error output
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::BrightBlue.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::BrightBlue.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(Parser)]
#[command(
    name = "u50",
    version,
    about = "An opinionated unification of Harvard's CS50 cli toolset.",
    propagate_version = true,
    styles = STYLES
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Minimum level of log messages to print
    #[arg(long, value_enum, global = true, default_value_t = LogLevel::Info, value_name = "LEVEL")]
    pub log: LogLevel,
}

#[derive(Subcommand)]
enum Commands {
    /// Submit a problem
    Submit {
        /// Prescribed identifier of work to submit
        slug: String,

        /// Agree to CS50's course policy on academic honesty, including its restrictions on AI use
        #[arg(long)]
        agree: bool,
    },

    /// Check a program's correctness
    Check {
        /// Prescribed identifier of work to check
        slug: String,

        // /// Run checks locally instead of uploading to cs50
        // #[arg(short, long)]
        // local: bool,
        /// Output mode of results
        #[arg(short, long, value_enum, default_value_t = CheckOutputMode::Ansi)]
        output: CheckOutputMode,

        /// Target specific checks to run
        #[arg(long)]
        target: Option<Vec<String>>,

        /// File to write output to
        #[arg(long, value_name = "FILE")]
        output_file: Option<PathBuf>,
    },

    /// Check the code's style against CS50's style guide
    Style {
        /// File or directory to check
        file: Vec<PathBuf>,

        /// Output mode of results
        #[arg(short, long, value_enum, default_value_t = StyleOutputMode::Unified)]
        output: StyleOutputMode,

        /// Rewrite files in-place (Beware!)
        #[arg(short, long)]
        write: bool,

        /// Paths/patterns to be ignored
        #[arg(long, value_name = "PATTERN")]
        ignore: Option<String>,
    },

    /// Information about plugins
    Plugins {
        /// List all available plugins
        #[arg(short, long)]
        list: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Info => Self::INFO,
            LogLevel::Warning => Self::WARN,
            LogLevel::Error => Self::ERROR,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum StyleOutputMode {
    Split,
    Unified,
    Score,
    Json,
    Html,
    Format,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CheckOutputMode {
    Ansi,
    Json,
    Html,
}
