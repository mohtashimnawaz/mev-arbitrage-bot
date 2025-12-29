use clap::{Parser, Subcommand};
use tracing::{info, error};

/// MEV Arbitrage Bot CLI
#[derive(Parser, Debug)]
#[command(name = "mev-bot")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the bot (stub)
    Run,
    /// Simulate / backtest (stub)
    Simulate,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => {
            info!("Starting bot (stub)...");
            if let Err(e) = mev_arbitrage_bot::run().await {
                error!(%e, "Bot failed");
            }
        }
        Commands::Simulate => {
            info!("Running simulator (stub)...");
            if let Err(e) = mev_arbitrage_bot::simulate().await {
                error!(%e, "Simulator failed");
            }
        }
    }
}
