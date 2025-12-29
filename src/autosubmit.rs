use anyhow::{Result, Context};
use std::time::Duration;
use ethers_core::types::{Bytes, H256};
use ethers_providers::{Provider, Http, Middleware};
use crate::executor::RelayClient;
use tokio::time::sleep;

/// Simple autosubmitter / monitor with configurable timeouts and backoff.
pub struct AutosubmitConfig {
    pub max_retries: usize,
    pub poll_interval_secs: u64,
    pub max_wait_secs: u64,
    pub kill_switch_max_gas_wei: Option<u128>,
}

impl Default for AutosubmitConfig {
    fn default() -> Self {
        Self { max_retries: 3, poll_interval_secs: 2, max_wait_secs: 30, kill_switch_max_gas_wei: None }
    }
}

pub struct Autosubmitter {
    pub config: AutosubmitConfig,
    pub rpc_url: String,
}

impl Autosubmitter {
    pub fn new(rpc_url: String, config: AutosubmitConfig) -> Self {
        Self { rpc_url, config }
    }

    /// Submit via relay if available, fallback to direct provider submission.
    /// Then monitor for inclusion by polling the provider for each tx hash.
    pub async fn submit_and_monitor(&self, signed_blob: &[Vec<u8>], relay: &RelayClient) -> Result<Vec<serde_json::Value>> {
        // Try relay first
        // track the last error if needed (currently unused)
        let mut _last_err: Option<anyhow::Error> = None;
        if let Ok(resp) = relay.submit_flashbots_bundle(signed_blob, None).await {
            tracing::info!("submitted to relay: {:?}", resp);
        } else {
            tracing::warn!("relay submission failed or not configured; falling back to provider");
        }

        // Fallback direct submission: send raw txs sequentially and monitor receipts
        let provider = Provider::<Http>::try_from(self.rpc_url.as_str()).context("invalid rpc url")?;

        let mut tx_hashes: Vec<H256> = Vec::new();
        for raw in signed_blob.iter() {
            let bytes = Bytes::from(raw.clone());
            let tx_hash = H256::from(ethers_core::utils::keccak256(&bytes));
            // attempt submission; if already sent by relay it may still not be present yet
            if let Err(e) = provider.send_raw_transaction(bytes.clone()).await {
                tracing::warn!("direct send_raw_transaction failed: {:?}", e);
                // continue; we'll still poll the expected hash
            }
            tx_hashes.push(tx_hash);
        }

        // Poll receipts
        let mut receipts_json: Vec<serde_json::Value> = Vec::new();
        let mut attempts = 0usize;
        let max_attempts = (self.config.max_wait_secs / self.config.poll_interval_secs) as usize;

        loop {
            attempts += 1;
            for h in tx_hashes.iter() {
                if let Ok(Some(receipt)) = provider.get_transaction_receipt(*h).await {
                    receipts_json.push(serde_json::to_value(&receipt).unwrap_or_default());
                }
            }
            if receipts_json.len() == tx_hashes.len() {
                return Ok(receipts_json);
            }
            if attempts >= max_attempts {
                // attempt re-submissions up to max_retries
                if self.config.max_retries == 0 {
                    return Err(anyhow::anyhow!("timed out waiting for inclusion and no retries configured"));
                }
                // Log a message about attempted resubmission
                tracing::warn!("inclusion not seen; attempting resubmission (retries left)");
                // One resubmission attempt: re-broadcast all raw txs
                let mut retry_count = 0usize;
                while retry_count < self.config.max_retries {
                    for raw in signed_blob.iter() {
                        let b = Bytes::from(raw.clone());
                        let _ = provider.send_raw_transaction(b).await;
                    }
                    retry_count += 1;
                    sleep(Duration::from_secs(1)).await;
                }
                // Final poll after resubmission
                attempts = 0;
            }
            sleep(Duration::from_secs(self.config.poll_interval_secs)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::build_eip1559_tx;
    use crate::signer::BasicEnvSigner;
    use ethers_core::types::{U256, Address, transaction::eip2718::TypedTransaction};

    #[tokio::test]
    #[ignore]
    async fn autosubmit_falls_back_to_provider_and_polls_receipts() {
        // This test requires ANVIL_RPC_URL and PRIVATE_KEY to be present
        let rpc = std::env::var("ANVIL_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        let private = match std::env::var("PRIVATE_KEY") {
            Ok(v) => v,
            Err(_) => {
                eprintln!("Skipping autosubmit test: set PRIVATE_KEY and run Anvil");
                return;
            }
        };

        let provider = Provider::<Http>::try_from(rpc.as_str()).unwrap();
        let chain_id = provider.get_chainid().await.unwrap().as_u64();
        let tx = build_eip1559_tx(
            U256::from(0u64),
            Address::zero(),
            U256::from(0u64),
            Bytes::from(vec![]),
            U256::from(21000u64),
            U256::from(1_000_000_000u64),
            U256::from(100_000_000_000u64),
            chain_id,
        );

        use crate::signer::Signer as _; // bring trait into scope for the method
        let signer = BasicEnvSigner::from_secret(private);
        let signed = signer.sign_typed_transaction(&tx).await.unwrap();
        let signed_blob = vec![signed];

        let autosub = Autosubmitter::new(rpc.clone(), AutosubmitConfig::default());
        let rc = crate::executor::RelayClient::without_relay().unwrap();
        let res = autosub.submit_and_monitor(&signed_blob, &rc).await.unwrap();
        assert!(res.len() > 0);
    }
}
