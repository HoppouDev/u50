use clap::Parser;

mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::from(cli.log))
        .with_writer(std::io::stderr)
        .init();

    Ok(())
}
