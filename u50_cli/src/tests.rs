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
    let Some(Command::Check(args)) = cli.command else {
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
    let Some(Command::Check(args)) = cli.command else {
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
    let Some(Command::Style(args)) = cli.command else {
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
fn style_fix_parses() {
    let cli = Cli::try_parse_from(["u50", "style", "a.c", "--fix"]).expect("valid arguments");
    let Some(Command::Style(args)) = cli.command else {
        panic!("expected style subcommand");
    };
    assert!(args.fix);
    assert!(!args.dry_run);
}

#[test]
fn style_fix_dry_run_parses() {
    let cli = Cli::try_parse_from(["u50", "style", "a.c", "--fix", "--dry-run"]).expect("valid");
    let Some(Command::Style(args)) = cli.command else {
        panic!("expected style subcommand");
    };
    assert!(args.fix);
    assert!(args.dry_run);
}

#[test]
fn style_fix_conflicts_with_output() {
    let result = Cli::try_parse_from(["u50", "style", "a.c", "--fix", "-o", "json"]);
    assert!(result.is_err(), "--fix must conflict with -o/--output");
}

#[test]
fn style_dry_run_requires_fix() {
    let result = Cli::try_parse_from(["u50", "style", "a.c", "--dry-run"]);
    assert!(result.is_err(), "--dry-run must require --fix");
}

#[test]
fn root_status_parses_without_subcommand() {
    let cli = Cli::try_parse_from(["u50", "--status"]).expect("valid arguments");
    assert!(cli.status);
    assert!(cli.command.is_none());
}

#[test]
fn root_setup_parses_without_subcommand() {
    let cli = Cli::try_parse_from(["u50", "--setup"]).expect("valid arguments");
    assert!(cli.setup);
    assert!(cli.command.is_none());
}

#[test]
fn style_setup_is_unknown_argument() {
    // `--setup` moved to the root command; on `style` it is an
    // unknown-argument error.
    let result = Cli::try_parse_from(["u50", "style", "--setup"]);
    assert!(result.is_err(), "style --setup must be rejected by clap");
}

#[test]
fn style_list_is_unknown_argument() {
    // `--list` moved to the root command as `--status`; on `style` it is
    // an unknown-argument error.
    let result = Cli::try_parse_from(["u50", "style", "--list"]);
    assert!(result.is_err(), "style --list must be rejected by clap");
}

#[test]
fn setup_with_subcommand_is_usage_error() {
    let cli = Cli::try_parse_from(["u50", "--setup", "style", "foo.c"]).expect("parses");
    assert!(cli.setup);
    assert!(cli.command.is_some());
    assert_eq!(run(cli).expect("usage errors return Ok"), ExitCode::from(2));
}

#[test]
fn status_with_setup_is_usage_error() {
    let cli = Cli::try_parse_from(["u50", "--status", "--setup"]).expect("parses");
    assert_eq!(run(cli).expect("usage errors return Ok"), ExitCode::from(2));
}

#[test]
fn status_with_subcommand_is_usage_error() {
    let cli = Cli::try_parse_from(["u50", "--status", "style", "foo.c"]).expect("parses");
    assert_eq!(run(cli).expect("usage errors return Ok"), ExitCode::from(2));
}

#[test]
fn bare_invocation_is_usage_error() {
    let cli = Cli::try_parse_from(["u50"]).expect("parses");
    assert!(cli.command.is_none());
    assert_eq!(run(cli).expect("usage errors return Ok"), ExitCode::from(2));
}

#[test]
fn style_without_files_is_usage_error() {
    let cli = Cli::try_parse_from(["u50", "style"]).expect("parses");
    assert_eq!(run(cli).expect("usage errors return Ok"), ExitCode::from(2));
}

#[test]
fn submit_parses_flags() {
    let cli = Cli::try_parse_from(["u50", "submit", "pset1", "--yes", "--dry-run"])
        .expect("valid arguments");
    let Some(Command::Submit(args)) = cli.command else {
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
    let Some(Command::Check(args)) = cli.command else {
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
