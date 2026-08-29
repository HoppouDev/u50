#![warn(clippy::pedantic)]

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use tracing_subscriber::filter::LevelFilter;

#[derive(Parser)]
#[command(
    name = "u50",
    version,
    about = "unified CS50 tooling: check, style, submit"
)]
struct Cli {
    #[command(flatten)]
    globals: Globals,

    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Debug)]
struct Globals {
    /// Increase log verbosity (repeat for more detail)
    #[arg(short = 'v', long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress log output
    #[arg(short = 'q', long, global = true)]
    quiet: bool,

    /// Override the log level explicitly (takes precedence over -v/-q)
    #[arg(long, value_enum, global = true)]
    log_level: Option<LogLevel>,

    /// Color output mode (auto honors `NO_COLOR`)
    #[arg(long, value_enum, default_value_t = Color::Auto, global = true)]
    color: Color,
}

#[derive(Subcommand)]
enum Command {
    /// Run checks against student code
    Check(CheckArgs),
    /// Check code style
    Style(StyleArgs),
    /// Submit work to GitHub
    Submit(SubmitArgs),
}

#[derive(Args, Debug)]
struct CheckArgs {
    /// Problem slug (e.g. cs50/problems/2018/x/caesar)
    slug: String,

    /// Execution mode
    #[arg(long, value_enum, default_value_t = Mode::Online)]
    mode: Mode,

    /// Run only the named checks (plus their dependencies); repeatable
    #[arg(long = "target", value_name = "NAME")]
    targets: Vec<String>,

    /// Output formats; repeatable
    #[arg(
        short = 'o',
        long = "output",
        value_enum,
        default_values_t = vec![OutputFormat::Ansi]
    )]
    outputs: Vec<OutputFormat>,

    /// Write output to a file instead of stdout
    #[arg(long, value_name = "PATH")]
    output_file: Option<std::path::PathBuf>,
}

#[derive(Args, Debug)]
struct StyleArgs {
    /// Files to style-check
    files: Vec<std::path::PathBuf>,

    /// Diff output format
    #[arg(short = 'o', long, value_enum, default_value_t = StyleOutput::Character)]
    output: StyleOutput,

    /// Rewrite files in place with style50 formatting
    #[arg(long, conflicts_with = "output")]
    fix: bool,

    /// Show what would change without writing (requires --fix)
    #[arg(long, requires = "fix")]
    dry_run: bool,
}

// The bool fields mirror the CLI's `--yes`/`--ssh`/`--dry-run`/`--logout`
// options one-to-one, so the bool count is inherent to the interface.
#[allow(clippy::struct_excessive_bools)]
#[derive(Args, Debug)]
struct SubmitArgs {
    /// Problem slug (e.g. pset1)
    slug: String,

    /// Skip the confirmation prompt
    #[arg(long)]
    yes: bool,

    /// Force SSH transport
    #[arg(long)]
    ssh: bool,

    /// Show what would be submitted without pushing
    #[arg(long)]
    dry_run: bool,

    /// Log out of the current session
    #[arg(long)]
    logout: bool,
}

#[derive(ValueEnum, Clone, Debug, Copy, PartialEq, Eq)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(ValueEnum, Clone, Debug, Copy, PartialEq, Eq)]
enum Color {
    Auto,
    Always,
    Never,
}

#[derive(ValueEnum, Clone, Debug, Copy, PartialEq, Eq)]
enum Mode {
    Online,
    Local,
    Offline,
    Dev,
}

#[derive(ValueEnum, Clone, Debug, Copy, PartialEq, Eq)]
enum OutputFormat {
    Ansi,
    Html,
    Json,
}

#[derive(ValueEnum, Clone, Debug, Copy, PartialEq, Eq)]
enum StyleOutput {
    Character,
    Split,
    Unified,
    Json,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let no_color = std::env::var_os("NO_COLOR").is_some();
    init_tracing(&cli, no_color);
    let result = match cli.command {
        Command::Check(args) => {
            let outputs = args.outputs.iter().map(|o| map_output_format(*o)).collect();
            u50_check::run(&u50_check::Request {
                slug: args.slug,
                mode: map_mode(args.mode),
                targets: args.targets,
                outputs,
                output_file: args.output_file,
            })
            .map(|()| ExitCode::SUCCESS)
        }
        Command::Style(args) => {
            let request = u50_style::Request {
                files: args.files,
                output: map_style_output(args.output),
                color: resolve_ansi(cli.globals.color, no_color),
            };
            if args.fix {
                // In-place fix: 3 on any per-file error (incl. write
                // failures); 1 only for a dry run that would change
                // something (check-style convention); 0 when every file was
                // fixed or was already clean.
                let report = u50_style::fix(&request, args.dry_run);
                Ok(if report.has_errors() {
                    ExitCode::from(3)
                } else if args.dry_run && !report.clean() {
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                })
            } else {
                let report = u50_style::run(&request);
                Ok(if report.has_errors() {
                    ExitCode::from(3)
                } else if report.clean() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                })
            }
        }
        Command::Submit(args) => u50_submit::run(&u50_submit::Request {
            slug: args.slug,
            yes: args.yes,
            ssh: args.ssh,
            dry_run: args.dry_run,
            logout: args.logout,
        })
        .map(|()| ExitCode::SUCCESS),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            // Style violations exit 1 and per-file style errors exit 3 (both
            // mapped from the Report above); check failures are still TODO;
            // 3 remains the infrastructure-error code.
            ExitCode::from(3)
        }
    }
}

/// Maps the CLI's `Mode` onto the `u50_check` library type.
fn map_mode(mode: Mode) -> u50_check::Mode {
    match mode {
        Mode::Online => u50_check::Mode::Online,
        Mode::Local => u50_check::Mode::Local,
        Mode::Offline => u50_check::Mode::Offline,
        Mode::Dev => u50_check::Mode::Dev,
    }
}

/// Maps the CLI's `OutputFormat` onto the `u50_check` library type.
fn map_output_format(format: OutputFormat) -> u50_check::Output {
    match format {
        OutputFormat::Ansi => u50_check::Output::Ansi,
        OutputFormat::Html => u50_check::Output::Html,
        OutputFormat::Json => u50_check::Output::Json,
    }
}

/// Maps the CLI's `StyleOutput` onto the `u50_style` library type.
fn map_style_output(output: StyleOutput) -> u50_style::Output {
    match output {
        StyleOutput::Character => u50_style::Output::Character,
        StyleOutput::Split => u50_style::Output::Split,
        StyleOutput::Unified => u50_style::Output::Unified,
        StyleOutput::Json => u50_style::Output::Json,
    }
}

/// Resolves the effective log level: explicit flag wins, then `-q`, then
/// the `-v` count (0 -> WARN, 1 -> INFO, 2 -> DEBUG, 3+ -> TRACE).
fn resolve_level(quiet: bool, verbose: u8, explicit: Option<LogLevel>) -> LevelFilter {
    if let Some(level) = explicit {
        match level {
            LogLevel::Trace => LevelFilter::TRACE,
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Error => LevelFilter::ERROR,
        }
    } else if quiet {
        LevelFilter::OFF
    } else {
        match verbose {
            0 => LevelFilter::WARN,
            1 => LevelFilter::INFO,
            2 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        }
    }
}

/// Resolves whether ANSI colors are enabled; in `Auto` mode, honors
/// `NO_COLOR` (probed by the caller to keep this helper pure).
fn resolve_ansi(color: Color, no_color_set: bool) -> bool {
    match color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => !no_color_set,
    }
}

fn init_tracing(cli: &Cli, no_color: bool) {
    let level = resolve_level(
        cli.globals.quiet,
        cli.globals.verbose,
        cli.globals.log_level,
    );
    let ansi = resolve_ansi(cli.globals.color, no_color);

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(ansi)
        .init();
}

#[cfg(test)]
mod tests;
