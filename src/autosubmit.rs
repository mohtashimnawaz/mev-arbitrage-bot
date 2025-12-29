use anyhow::{Result, Context};
use std::time::Duration;
use ethers_core::types::{Bytes, H256, U256, transaction::eip2718::TypedTransaction};
use ethers_providers::{Provider, Http, Middleware};
use crate::executor::RelayClient;
use tokio::time::sleep;
use tracing::instrument;

/// Simple autosubmitter / monitor with configurable timeouts and backoff.
pub struct AutosubmitConfig {
    pub max_retries: usize,
    pub poll_interval_secs: u64,
    pub max_wait_secs: u64,
    /// Bump factor applied to gas prices on each re-submission attempt (e.g., 1.25)
    pub bump_factor: f64,
    /// Maximum number of bump attempts
    pub max_bumps: usize,
    /// Maximum allowed worst-case gas spend in wei (kill switch)
    pub kill_switch_max_gas_wei: Option<u128>,
    /// Maximum allowed net loss (wei) relative to expected PnL (kill switch)
    pub kill_switch_max_loss_wei: Option<i128>,
}

impl Default for AutosubmitConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            poll_interval_secs: 2,
            max_wait_secs: 30,
            bump_factor: 1.25,
            max_bumps: 3,
            kill_switch_max_gas_wei: None,
            kill_switch_max_loss_wei: None,
        }
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
        // direct path without rebidding/signing capability
        self.submit_and_monitor_with_rebump(None, None, signed_blob.to_vec(), relay, None).await
    }

    /// Extended submission that supports optional unsigned transactions + signer to allow gas-bumping
    /// and re-signing on retries. `expected_pnl` is an optional per-tx expected PnL vector (in wei) used for kill-switch checks.
    #[instrument(skip(self, unsigned_txs, signer, relay, expected_pnl))]
    pub async fn submit_and_monitor_with_rebump(
        &self,
        unsigned_txs: Option<&[TypedTransaction]>,
        signer: Option<std::sync::Arc<dyn crate::signer::Signer>>,
        mut signed_blob: Vec<Vec<u8>>,
        relay: &RelayClient,
        expected_pnl: Option<&[i128]>,
    ) -> Result<Vec<serde_json::Value>> {
        // Try relay first
        if let Ok(resp) = relay.submit_flashbots_bundle(&signed_blob, None).await {
            tracing::info!("submitted to relay: {:?}", resp);
            #[cfg(feature = "with-metrics")]
            {
                metrics::increment_counter!("autosubmit.submissions.relay", 1);
            }
        } else {
            tracing::warn!("relay submission failed or not configured; falling back to provider");
        }

        // Fallback direct submission: send raw txs sequentially and monitor receipts
        let provider = Provider::<Http>::try_from(self.rpc_url.as_str()).context("invalid rpc url")?;

        // Compute expected tx hashes (keccak256 of signed raw bytes)
        let mut tx_hashes: Vec<H256> = Vec::new();
        for raw in signed_blob.iter() {
            let bytes = Bytes::from(raw.clone());
            let tx_hash = H256::from(ethers_core::utils::keccak256(&bytes));
            // attempt submission; if already sent by relay it may still not be present yet
            if let Err(e) = provider.send_raw_transaction(bytes.clone()).await {
                tracing::warn!("direct send_raw_transaction failed: {:?}", e);
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
                    let _ = receipts_json.push(serde_json::to_value(&receipt).unwrap_or_default());
                    #[cfg(feature = "with-metrics")]
                    {
                        metrics::increment_counter!("autosubmit.inclusions", 1);
                    }
                }
            }
            if receipts_json.len() == tx_hashes.len() {
                return Ok(receipts_json);
            }
            if attempts >= max_attempts {
                // attempt re-submissions up to max_retries with optional gas bumping
                if self.config.max_retries == 0 {
                    return Err(anyhow::anyhow!("timed out waiting for inclusion and no retries configured"));
                }
                tracing::warn!("inclusion not seen; attempting resubmission (retries left)");

                // If we have unsigned txs and a signer, attempt gas bump re-signing
                if let (Some(unsigned), Some(signer_arc)) = (unsigned_txs, signer.as_ref()) {
                    for bump_idx in 0..self.config.max_bumps {
                        let factor = self.config.bump_factor.powi(bump_idx as i32 + 1);
                        tracing::info!("attempting gas bump {} (factor {:.3})", bump_idx + 1, factor);

                        // Apply kill-switch: estimate worst-case gas for this bump
                        let mut worst_case_cost: u128 = 0u128;
                        for tx in unsigned.iter() {
                            // Match on TypedTransaction variants to extract fields
                            match tx.clone() {
                                TypedTransaction::Eip1559(req) => {
                                    let gas_limit = req.gas.unwrap_or(U256::from(21000u64)).as_u128();
                                    let base_price = req.max_fee_per_gas.map(|m| m.as_u128()).unwrap_or(0u128);
                                    let new_price = ((base_price as f64) * factor) as u128;
                                    worst_case_cost = worst_case_cost.saturating_add(gas_limit.saturating_mul(new_price));
                                }
                                TypedTransaction::Legacy(req) => {
                                    let gas_limit = req.gas.unwrap_or(U256::from(21000u64)).as_u128();
                                    let base_price = req.gas_price.map(|p| p.as_u128()).unwrap_or(0u128);
                                    let new_price = ((base_price as f64) * factor) as u128;
                                    worst_case_cost = worst_case_cost.saturating_add(gas_limit.saturating_mul(new_price));
                                }
                                _ => {
                                    // Unknown tx type: assume minimal gas and zero price
                                    worst_case_cost = worst_case_cost.saturating_add(21000u128.saturating_mul(0u128));
                                }
                            }
                        }

                        if let Some(max_gas) = self.config.kill_switch_max_gas_wei {
                            if worst_case_cost > max_gas {
                                tracing::error!("kill-switch triggered: worst-case gas {} > max allowed {}", worst_case_cost, max_gas);
                                return Err(anyhow::anyhow!("kill-switch: worst-case gas exceeds allowed threshold"));
                            }
                        }

                        // if expected_pnl provided, ensure projected loss not exceeding threshold
                        if let Some(pnls) = expected_pnl {
                            let mut total_pnl: i128 = 0;
                            for v in pnls.iter() { total_pnl = total_pnl.saturating_add(*v); }
                            let projected_loss = (worst_case_cost as i128).saturating_sub(total_pnl);
                            if let Some(max_loss) = self.config.kill_switch_max_loss_wei {
                                if projected_loss > max_loss {
                                    tracing::error!("kill-switch triggered: projected loss {} > max allowed {}", projected_loss, max_loss);
                                    return Err(anyhow::anyhow!("kill-switch: projected loss exceeds allowed threshold"));
                                }
                            }
                        }

                        // Construct bumped unsigned txs
                        let mut bumped_signed_blob: Vec<Vec<u8>> = Vec::new();
                        for tx in unsigned.iter() {
                            match tx.clone() {
                                TypedTransaction::Eip1559(req) => {
                                    let mfp = req.max_fee_per_gas.unwrap_or_else(|| U256::from(0u64));
                                    let new_mfp = U256::from(((mfp.as_u128() as f64) * factor) as u128);
                                    let mpp = req.max_priority_fee_per_gas.unwrap_or_else(|| U256::from(0u64));
                                    let new_mpp = U256::from(((mpp.as_u128() as f64) * factor) as u128);
                                    let mut req2 = req.clone();
                                    req2 = req2.max_fee_per_gas(new_mfp);
                                    req2 = req2.max_priority_fee_per_gas(new_mpp);
                                    let t2 = TypedTransaction::Eip1559(req2);
                                    let signed = signer_arc.sign_typed_transaction(&t2).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                                    bumped_signed_blob.push(signed);
                                }
                                TypedTransaction::Legacy(req) => {
                                    let gp = req.gas_price.unwrap_or_else(|| U256::from(0u64));
                                    let new_gp = U256::from(((gp.as_u128() as f64) * factor) as u128);
                                    let mut req2 = req.clone();
                                    req2 = req2.gas_price(new_gp);
                                    let t2 = TypedTransaction::Legacy(req2);
                                    let signed = signer_arc.sign_typed_transaction(&t2).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                                    bumped_signed_blob.push(signed);
                                }
                                other => {
                                    // For unknown typed txs, attempt to sign as-is
                                    let signed = signer_arc.sign_typed_transaction(&other).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                                    bumped_signed_blob.push(signed);
                                }
                            }
                        }

                        // Broadcast bumped submissions
                        for raw in bumped_signed_blob.iter() {
                            let b = Bytes::from(raw.clone());
                            let _ = provider.send_raw_transaction(b).await;
                        }

                        #[cfg(feature = "with-metrics")]
                        {
                            metrics::increment_counter!("autosubmit.resubmissions", 1);
                        }

                        // After broadcasting, reset attempts and watch again
                        attempts = 0;
                        // replace tracked tx hashes to new ones
                        tx_hashes.clear();
                        for raw in bumped_signed_blob.iter() {
                            let bytes = Bytes::from(raw.clone());
                            let tx_hash = H256::from(ethers_core::utils::keccak256(&bytes));
                            tx_hashes.push(tx_hash);
                        }

                        // Save the bumped signed blob for potential further bumps
                        signed_blob = bumped_signed_blob;
                        // Wait a bit to allow propagation
                        sleep(Duration::from_secs(1)).await;
                    }

                    // All bumps exhausted
                    return Err(anyhow::anyhow!("exhausted gas bump attempts without inclusion"));
                } else {
                    // No signer available — perform direct re-broadcasts only
                    let mut retry_count = 0usize;
                    while retry_count < self.config.max_retries {
                        for raw in signed_blob.iter() {
                            let b = Bytes::from(raw.clone());
                            let _ = provider.send_raw_transaction(b).await;
                        }
                        retry_count += 1;
                        sleep(Duration::from_secs(1)).await;
                    }
                    attempts = 0;
                }
            }
            sleep(Duration::from_secs(self.config.poll_interval_secs)).await;
        }
    }
}
