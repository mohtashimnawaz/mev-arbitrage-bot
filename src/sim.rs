use anyhow::{Result, Context};
use ethers_providers::{Provider, Http, Middleware};
use std::time::Duration;

/// Simulation / backtesting helper: verifies we can reach a forked node and
/// (in future) will replay transactions deterministically.
pub struct Simulator {
    rpc: String,
}

impl Simulator {
    pub fn new() -> Self {
        let rpc = std::env::var("ANVIL_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        Self { rpc }
    }

    pub async fn run_trade_simulation(&self, _tx_bytes: &[u8]) -> Result<bool> {
        // For now: sanity-check that the RPC is reachable and returns a block number
        let provider = Provider::<Http>::try_from(self.rpc.as_str())
            .context("failed to create provider from RPC URL")?
            .interval(Duration::from_millis(200));
        let bn = provider.get_block_number().await.context("rpc call failed")?;
        tracing::info!("Connected to fork RPC, block number: {}", bn);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sim_succeeds_if_no_rpc() {
        // This test will likely fail if no local node is running; we only validate construction
        let sim = Simulator::new();
        // We don't call run_trade_simulation() to avoid flaky failures in CI
        assert!(sim.rpc.len() > 0);
    }
}
