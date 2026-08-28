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
        Command::Style(args) => u50_style::run(&u50_style::Request {
            files: args.files,
            output: map_style_output(args.output),
            color: resolve_ansi(cli.globals.color, no_color),
        })
        .map(|report| {
            if report.clean() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }),
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
            // Style violations exit 1 (mapped from the Report above); check
            // failures are still TODO; 3 remains the infrastructure-error code.
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
mod tests {
    use super::*;

    #[test]
    fn check_parses_full_form() {
        let cli = Cli::try_parse_from([
            "u50",
            "check",
            "cs50/problems/2018/x/caesar",
            "--mode",
            "local",
            "-o",
            "json",
            "--target",
            "foo",
        ])
        .expect("valid arguments");
        let Command::Check(args) = cli.command else {
            panic!("expected check subcommand");
        };
        assert_eq!(args.slug, "cs50/problems/2018/x/caesar");
        assert!(matches!(args.mode, Mode::Local));
        assert_eq!(args.outputs, vec![OutputFormat::Json]);
        assert_eq!(args.targets, vec!["foo".to_owned()]);
        assert!(args.output_file.is_none());
    }

    #[test]
    fn check_defaults() {
        let cli = Cli::try_parse_from(["u50", "check", "some/slug"]).expect("valid arguments");
        let Command::Check(args) = cli.command else {
            panic!("expected check subcommand");
        };
        assert!(matches!(args.mode, Mode::Online));
        assert_eq!(args.outputs, vec![OutputFormat::Ansi]);
        assert!(args.targets.is_empty());
    }

    #[test]
    fn style_parses_files_and_output() {
        let cli = Cli::try_parse_from(["u50", "style", "a.c", "b.c", "-o", "unified"])
            .expect("valid arguments");
        let Command::Style(args) = cli.command else {
            panic!("expected style subcommand");
        };
        assert_eq!(
            args.files,
            vec![
                std::path::PathBuf::from("a.c"),
                std::path::PathBuf::from("b.c")
            ]
        );
        assert!(matches!(args.output, StyleOutput::Unified));
    }

    #[test]
    fn submit_parses_flags() {
        let cli = Cli::try_parse_from(["u50", "submit", "pset1", "--yes", "--dry-run"])
            .expect("valid arguments");
        let Command::Submit(args) = cli.command else {
            panic!("expected submit subcommand");
        };
        assert_eq!(args.slug, "pset1");
        assert!(args.yes);
        assert!(args.dry_run);
        assert!(!args.ssh);
        assert!(!args.logout);
    }

    #[test]
    fn global_flag_after_subcommand() {
        let cli = Cli::try_parse_from(["u50", "check", "slug", "-vv"]).expect("valid arguments");
        assert_eq!(cli.globals.verbose, 2);
        assert!(!cli.globals.quiet);
        assert!(cli.globals.log_level.is_none());
        assert!(matches!(cli.globals.color, Color::Auto));
    }

    #[test]
    fn check_output_file_parses() {
        let cli = Cli::try_parse_from(["u50", "check", "slug", "--output-file", "/tmp/u50x.out"])
            .expect("valid arguments");
        let Command::Check(args) = cli.command else {
            panic!("expected check subcommand");
        };
        assert_eq!(
            args.output_file,
            Some(std::path::PathBuf::from("/tmp/u50x.out"))
        );
    }

    #[test]
    fn mode_mapping_covers_every_variant() {
        let cases = [
            (Mode::Online, u50_check::Mode::Online),
            (Mode::Local, u50_check::Mode::Local),
            (Mode::Offline, u50_check::Mode::Offline),
            (Mode::Dev, u50_check::Mode::Dev),
        ];
        for (cli, lib) in cases {
            assert_eq!(map_mode(cli), lib, "mode mapping broken for {cli:?}");
        }
    }

    #[test]
    fn output_format_mapping_covers_every_variant() {
        let cases = [
            (OutputFormat::Ansi, u50_check::Output::Ansi),
            (OutputFormat::Html, u50_check::Output::Html),
            (OutputFormat::Json, u50_check::Output::Json),
        ];
        for (cli, lib) in cases {
            assert_eq!(
                map_output_format(cli),
                lib,
                "output-format mapping broken for {cli:?}"
            );
        }
    }

    #[test]
    fn style_output_mapping_covers_every_variant() {
        let cases = [
            (StyleOutput::Character, u50_style::Output::Character),
            (StyleOutput::Split, u50_style::Output::Split),
            (StyleOutput::Unified, u50_style::Output::Unified),
            (StyleOutput::Json, u50_style::Output::Json),
        ];
        for (cli, lib) in cases {
            assert_eq!(
                map_style_output(cli),
                lib,
                "style-output mapping broken for {cli:?}"
            );
        }
    }

    #[test]
    fn resolve_level_explicit_beats_quiet_and_verbose() {
        assert_eq!(
            resolve_level(true, 3, Some(LogLevel::Error)),
            tracing_subscriber::filter::LevelFilter::ERROR
        );
        assert_eq!(
            resolve_level(false, 0, Some(LogLevel::Trace)),
            tracing_subscriber::filter::LevelFilter::TRACE
        );
    }

    #[test]
    fn resolve_level_quiet_gives_off() {
        assert_eq!(
            resolve_level(true, 0, None),
            tracing_subscriber::filter::LevelFilter::OFF
        );
        assert_eq!(
            resolve_level(true, 2, None),
            tracing_subscriber::filter::LevelFilter::OFF
        );
    }

    #[test]
    fn resolve_level_verbose_count() {
        assert_eq!(
            resolve_level(false, 0, None),
            tracing_subscriber::filter::LevelFilter::WARN
        );
        assert_eq!(
            resolve_level(false, 1, None),
            tracing_subscriber::filter::LevelFilter::INFO
        );
        assert_eq!(
            resolve_level(false, 2, None),
            tracing_subscriber::filter::LevelFilter::DEBUG
        );
        assert_eq!(
            resolve_level(false, 3, None),
            tracing_subscriber::filter::LevelFilter::TRACE
        );
        assert_eq!(
            resolve_level(false, 255, None),
            tracing_subscriber::filter::LevelFilter::TRACE
        );
    }

    #[test]
    fn resolve_ansi_color_modes_and_no_color() {
        assert!(resolve_ansi(Color::Always, true));
        assert!(resolve_ansi(Color::Always, false));
        assert!(!resolve_ansi(Color::Never, false));
        assert!(!resolve_ansi(Color::Never, true));
        assert!(!resolve_ansi(Color::Auto, true));
        assert!(resolve_ansi(Color::Auto, false));
    }
}
