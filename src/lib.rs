pub mod config;
pub mod data;
pub mod executor;
pub mod scanner;
pub mod signer;
pub mod sim;
pub mod tx;

use anyhow::Result;
use tracing::{info, warn};

pub async fn run() -> Result<()> {
    let cfg = config::Config::default();

    let md = data::MarketDataClient::new(cfg.rpc_urls.clone(), cfg.ws_urls.clone()).await?;
    md.start().await?;

    // Spawn a background task that subscribes to market data and runs the scanner
    tokio::spawn(async move {
        let mut rx = md.subscribe();
        let mut scanner = scanner::Scanner::new(8, 0.02); // 8-sample window; 2% threshold
        loop {
            match rx.recv().await {
                Ok(q) => {
                    if let Some(opp) = scanner.process_quote(&q) {
                        info!("Detected opportunity: {}", opp);
                    }
                }
                Err(e) => {
                    warn!("Market data recv error: {:?}", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    });

    info!("Bot started (background tasks running)");
    Ok(())
}

pub async fn simulate() -> Result<()> {
    // Simple simulation entrypoint; extend to run backtests
    let sim = sim::Simulator::new();
    let ok = sim.run_trade_simulation(&[]).await?;
    info!("Simulation finished: {}", ok);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_stub() {
        run().await.unwrap();
    }
}
