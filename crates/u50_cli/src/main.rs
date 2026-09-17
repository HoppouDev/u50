use clap::Parser;
use crossterm::style::Stylize;
use tracing::info;

use crate::cli::{Commands, PluginsCommands};

mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::from(cli.log))
        .with_writer(std::io::stderr)
        .init();

    #[allow(unused)]
    match &cli.command {
        Commands::Submit { slug, agree } => {
            info!("Submitting as slug \"{}\"", slug);
        }

        Commands::Check {
            slug,
            output,
            target,
            output_file,
        } => {
            info!("Checking correctness against slug \"{}\"", slug);
        }

        Commands::Style {
            file,
            output,
            write,
            ignore,
        } => {
            info!(
                "Checking style of path(s) [{}] against CS50's style guide",
                file.iter()
                    .map(|f| format!("\"{}\"", f.display()))
                    .collect::<Vec<String>>()
                    .join(", ")
            );
        }

        Commands::Plugins { command } => match command {
            PluginsCommands::List => {
                print_plugin_section(
                    "Formatter",
                    u50_tools::plugin::formatter::all()
                        .map(|plugin| plugin.display_name)
                        .collect::<Vec<_>>()
                        .as_slice(),
                );
                println!();
                print_plugin_section(
                    "Language",
                    u50_tools::plugin::language::all()
                        .map(|plugin| plugin.display_name)
                        .collect::<Vec<_>>()
                        .as_slice(),
                );
                println!();
                print_plugin_section(
                    "Resolver",
                    u50_tools::plugin::resolver::all()
                        .map(|plugin| plugin.display_name())
                        .collect::<Vec<_>>()
                        .as_slice(),
                );
                println!();
                print_plugin_section(
                    "Test",
                    u50_tools::plugin::test::all()
                        .map(|plugin| plugin.display_name)
                        .collect::<Vec<_>>()
                        .as_slice(),
                );
            }
        },
    }

    Ok(())
}

/// Prints one colored section of the plugin listing
fn print_plugin_section(title: &str, names: &[&str]) {
    println!("{}{}", title.bold().blue(), " Plugins:".bold().blue());
    if names.is_empty() {
        println!("{}", "(nothing registered)".dark_grey().italic());
    } else {
        for name in names {
            println!(" {} {}", "•".dark_grey(), name.white());
        }
    }
}
