use clap::Parser;

mod cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cli::Cli::parse();

    Ok(())
}
