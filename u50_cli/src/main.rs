#![warn(clippy::pedantic)]

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

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
    init_tracing(&cli);
    let result = match cli.command {
        Command::Check(args) => {
            let mode = match args.mode {
                Mode::Online => u50_check::Mode::Online,
                Mode::Local => u50_check::Mode::Local,
                Mode::Offline => u50_check::Mode::Offline,
                Mode::Dev => u50_check::Mode::Dev,
            };
            let outputs = args
                .outputs
                .iter()
                .map(|o| match o {
                    OutputFormat::Ansi => "ansi",
                    OutputFormat::Html => "html",
                    OutputFormat::Json => "json",
                })
                .map(str::to_owned)
                .collect();
            u50_check::run(&u50_check::Request {
                slug: args.slug,
                mode,
                targets: args.targets,
                outputs,
            })
        }
        Command::Style(args) => {
            let output = match args.output {
                StyleOutput::Character => u50_style::Output::Character,
                StyleOutput::Split => u50_style::Output::Split,
                StyleOutput::Unified => u50_style::Output::Unified,
                StyleOutput::Json => u50_style::Output::Json,
            };
            u50_style::run(&u50_style::Request {
                files: args.files,
                output,
            })
        }
        Command::Submit(args) => u50_submit::run(&u50_submit::Request {
            slug: args.slug,
            yes: args.yes,
            ssh: args.ssh,
            dry_run: args.dry_run,
            logout: args.logout,
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            // TODO: map check failures / style violations to exit code 1 once
            // the engines are implemented; 3 remains the infrastructure-error code.
            ExitCode::from(3)
        }
    }
}

fn init_tracing(cli: &Cli) {
    use tracing_subscriber::filter::LevelFilter;

    let level = if let Some(level) = cli.globals.log_level {
        match level {
            LogLevel::Trace => LevelFilter::TRACE,
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Error => LevelFilter::ERROR,
        }
    } else if cli.globals.quiet {
        LevelFilter::OFF
    } else {
        match cli.globals.verbose {
            0 => LevelFilter::WARN,
            1 => LevelFilter::INFO,
            2 => LevelFilter::DEBUG,
            _ => LevelFilter::TRACE,
        }
    };

    let ansi = match cli.globals.color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => std::env::var_os("NO_COLOR").is_none(),
    };

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
}
