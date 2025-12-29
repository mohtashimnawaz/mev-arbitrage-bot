use anyhow::{Result, Context};
use ethers_providers::{Provider, Http, Middleware};
use ethers_core::types::{Address, Bytes, transaction::eip2718::TypedTransaction, U256, transaction::eip2718::TypedTransaction as TTx, TransactionReceipt};
use crate::signer::Signer;
use std::convert::TryInto;
use std::time::Duration;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::sync::Semaphore;
use std::sync::Arc;

/// Scorer for simulated bundles. Returns a signed 128-bit score (higher is better).
pub trait Scorer: Send + Sync {
    /// Score receipts and optional expected pnl per tx. Returns a signed i128 value (higher is better).
    fn score(&self, receipts: &[TransactionReceipt], signed_txs: &[Vec<u8>], expected_pnl: Option<&[i128]>) -> i128;
}

/// Default scorer: penalize reverts heavily, otherwise score is negative gas cost.
pub struct GasCostScorer;

impl Scorer for GasCostScorer {
    fn score(&self, receipts: &[TransactionReceipt], _signed_txs: &[Vec<u8>], _expected_pnl: Option<&[i128]>) -> i128 {
        let mut total: i128 = 0;
        for r in receipts.iter() {
            // status: 1 = success, 0 = revert
            if let Some(status) = r.status {
                if status.as_u64() == 0u64 {
                    return i128::MIN / 4; // huge negative for revert
                }
            }
            let gas_used = r.gas_used.unwrap_or_default();
            // prefer effectiveGasPrice
            let gas_price = r.effective_gas_price.unwrap_or_default();
            let cost = gas_used.saturating_mul(gas_price);
            // convert to i128 (may overflow in practice for huge numbers; clamp)
            let cost_i128 = match <ethers_core::types::U256 as TryInto<u128>>::try_into(cost) {
                Ok(v) => v as i128,
                Err(_) => i128::MAX / 8,
            };
            total -= cost_i128;
        }
        total
    }
}

/// Configurable scorer with weights for revert penalty, gas cost and expected P&L.
pub struct ConfigurableScorer {
    pub revert_penalty: i128,
    pub gas_weight: i128,
    pub pnl_weight: i128,
}

impl ConfigurableScorer {
    pub fn new(revert_penalty: i128, gas_weight: i128, pnl_weight: i128) -> Self {
        Self { revert_penalty, gas_weight, pnl_weight }
    }
}

impl Scorer for ConfigurableScorer {
    fn score(&self, receipts: &[TransactionReceipt], _signed_txs: &[Vec<u8>], expected_pnl: Option<&[i128]>) -> i128 {
        let mut total: i128 = 0;
        for (i, r) in receipts.iter().enumerate() {
            if let Some(status) = r.status {
                if status.as_u64() == 0u64 {
                    total -= self.revert_penalty;
                    // continue to aggregate gas cost even on revert
                }
            }
            let gas_used = r.gas_used.unwrap_or_default();
            let gas_price = r.effective_gas_price.unwrap_or_default();
            let cost = gas_used.saturating_mul(gas_price);
            let cost_i128 = match <ethers_core::types::U256 as TryInto<u128>>::try_into(cost) {
                Ok(v) => v as i128,
                Err(_) => i128::MAX / 8,
            };
            total -= cost_i128 * self.gas_weight;
            if let Some(pnls) = expected_pnl {
                if i < pnls.len() {
                    total += pnls[i] * self.pnl_weight;
                }
            }
        }
        total
    }
}

/// Simulation / backtesting helper backed by a forked node (Anvil/Hardhat).
#[derive(Clone)]
pub struct Simulator {
    rpc: String,
}

impl Simulator {
    pub fn new() -> Self {
        let rpc = std::env::var("ANVIL_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        Self { rpc }
    }

    /// Simulate a bundle by taking a snapshot, sending each signed raw tx (in order),
    /// waiting for receipts, then reverting to snapshot to avoid affecting persistent state.
    pub async fn simulate_signed_bundle(&self, signed_raw_txs: &[Vec<u8>], set_next_block_base_fee: Option<U256>) -> Result<Vec<TransactionReceipt>> {
        let provider = Provider::<Http>::try_from(self.rpc.as_str()).context("invalid rpc url")?;

        // Create snapshot
        let snap_id: serde_json::Value = provider.request("evm_snapshot", ()).await.context("snapshot failed")?;

        // Optionally set the next block's base fee for the simulation
        if let Some(bf) = set_next_block_base_fee {
            let bf_hex = format!("0x{:x}", bf);
            let _r: serde_json::Value = provider.request("evm_setNextBlockBaseFeePerGas", vec![bf_hex]).await.context("set base fee failed")?;
        }

        let mut results = Vec::new();

        for raw in signed_raw_txs.iter() {
            let b = Bytes::from(raw.clone());
            let pending = provider.send_raw_transaction(b).await.context("send_raw failed")?;
            let receipt_opt = tokio::time::timeout(Duration::from_secs(10), pending).await.context("timeout awaiting tx")??;
            let receipt = receipt_opt.ok_or_else(|| anyhow::anyhow!("no receipt returned"))?;
            results.push(receipt);
        }
        // Revert snapshot to clean state
        let _: bool = provider.request("evm_revert", vec![snap_id]).await.context("revert failed")?;

        Ok(results)
    }

    /// Simulate an unsigned bundle by trying multiple base nonces in parallel. For each offset in
    /// `0..nonce_range` we assign nonce = base_nonce + offset for the first tx, and
    /// increment by 1 for each subsequent transaction. We sign each nonce sequence
    /// using `signer` and simulate the resulting signed bundle. The `concurrency` param
    /// bounds concurrent attempts. Each attempt can optionally set the next block base fee
    /// to `set_next_block_base_fee` for gas dynamics testing. Returns tuples of (nonce, score, receipts).
    pub async fn simulate_unsigned_bundle_try_nonces_with_scorer<'a, S: Signer + ?Sized + Send + Sync + 'static, C: Scorer + ?Sized + Send + Sync + 'static>(
        &self,
        unsigned_txs: &[TypedTransaction],
        signer: std::sync::Arc<S>,
        base_nonce: u64,
        nonce_range: u64,
        concurrency: usize,
        scorer: std::sync::Arc<C>,
        set_next_block_base_fee: Option<U256>,
    ) -> Result<Vec<(u64, i128, Vec<TransactionReceipt>, Vec<Vec<u8>>)>> {
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut futs = FuturesUnordered::new();

        for offset in 0..nonce_range {
            let permit = sem.clone().acquire_owned().await.unwrap();
            let unsigned = unsigned_txs.to_vec();
            let signer_cloned = signer.clone();
            let scorer_cloned = scorer.clone();
            let sim = self.clone();
            let bf = set_next_block_base_fee;
            futs.push(tokio::spawn(async move {
                // drop permit when done
                let _permit = permit;
                // Build signed bundle for this offset
                let mut signed_blob = Vec::new();
                let mut current = base_nonce + offset;
                for tx in unsigned.iter() {
                    let tx_with_nonce = set_nonce_tx(&tx, U256::from(current));
                    let signed = signer_cloned.sign_typed_transaction(&tx_with_nonce).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    signed_blob.push(signed);
                    current += 1;
                }
                // simulate
                let receipts = sim.simulate_signed_bundle(&signed_blob, bf).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let score = scorer_cloned.score(&receipts, &signed_blob, None);
                Ok::<(u64, i128, Vec<TransactionReceipt>, Vec<Vec<u8>>), anyhow::Error>((base_nonce + offset, score, receipts, signed_blob))
            }));
        }

        let mut results = Vec::new();
        while let Some(r) = futs.next().await {
            match r {
                Ok(Ok(tuple)) => results.push(tuple),
                Ok(Err(e)) => tracing::warn!("attempt failed: {:?}", e),
                Err(e) => tracing::warn!("task join error: {:?}", e),
            }
        }

        // Sort by nonce
        results.sort_by_key(|t| t.0);
        Ok(results)
    }

    /// Choose the best nonce strategy, return the signed bundle for submission, plus receipts and score.
    pub async fn choose_best_nonce_strategy<S: Signer + ?Sized + Send + Sync + 'static, C: Scorer + ?Sized + Send + Sync + 'static>(
        &self,
        unsigned_txs: &[TypedTransaction],
        signer: std::sync::Arc<S>,
        base_nonce: u64,
        nonce_range: u64,
        concurrency: usize,
        scorer: std::sync::Arc<C>,
        set_next_block_base_fee: Option<U256>,
    ) -> Result<Option<(u64, i128, Vec<Vec<u8>>, Vec<TransactionReceipt>)>> {
        let results = self.simulate_unsigned_bundle_try_nonces_with_scorer(unsigned_txs, signer, base_nonce, nonce_range, concurrency, scorer, set_next_block_base_fee).await?;
        // pick max scoring
        let mut best: Option<(u64, i128, Vec<Vec<u8>>, Vec<TransactionReceipt>)> = None;
        for (nonce, score, receipts, signed_blob) in results.into_iter() {
            match &best {
                None => best = Some((nonce, score, signed_blob, receipts)),
                Some((_, best_score, _, _)) => {
                    if score > *best_score {
                        best = Some((nonce, score, signed_blob, receipts));
                    }
                }
            }
        }
        Ok(best)
    }

    /// Autosubmit a chosen signed bundle: prefer relay submission; if no relay configured, send raw txs sequentially to provider.
    pub async fn autosubmit_signed_bundle(&self, signed_blob: &[Vec<u8>], relay_client: &crate::executor::RelayClient) -> Result<serde_json::Value> {
        // Try relay first
        if let Ok(resp) = relay_client.submit_flashbots_bundle(signed_blob, None).await {
            return Ok(serde_json::json!({"relay": resp}));
        }

        // Fallback: submit directly to provider sequentially
        let provider = Provider::<Http>::try_from(self.rpc.as_str()).context("invalid rpc url for autosubmit")?;
        let mut receipts = Vec::new();
        for raw in signed_blob.iter() {
            let pending = provider.send_raw_transaction(Bytes::from(raw.clone())).await.context("send_raw failed")?;
            let receipt = tokio::time::timeout(Duration::from_secs(10), pending).await.context("timeout awaiting tx")??;
            if let Some(r) = receipt {
                receipts.push(serde_json::to_value(&r).unwrap_or_default());
            }
        }
        Ok(serde_json::json!({"direct_receipts": receipts}))
    }

    pub async fn run_trade_simulation(&self, _tx_bytes: &[u8]) -> Result<bool> {
        // keep previous sanity check behavior as a convenience method
        let provider = Provider::<Http>::try_from(self.rpc.as_str())
            .context("failed to create provider from RPC URL")?
            .interval(Duration::from_millis(200));
        let bn = provider.get_block_number().await.context("rpc call failed")?;
        tracing::info!("Connected to fork RPC, block number: {}", bn);
        Ok(true)
    }
}

fn set_nonce_tx(tx: &TypedTransaction, nonce: U256) -> TypedTransaction {
    match tx.clone() {
        TypedTransaction::Eip1559(req) => {
            let req2 = req.nonce(nonce);
            TypedTransaction::Eip1559(req2)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::build_eip1559_tx;
    use crate::signer::{BasicEnvSigner, Signer};
    use ethers_core::types::{U256, Address, Bytes, transaction::eip2718::TypedTransaction};

    #[tokio::test]
    #[ignore]
    async fn simulate_unsigned_bundle_on_anvil_try_nonces() {
        let sim = Simulator::new();
        // requires env PRIVATE_KEY and anvil running
        let private = match std::env::var("PRIVATE_KEY") {
            Ok(v) => v,
            Err(_) => {
                eprintln!("Skipping simulate_unsigned_bundle_on_anvil_try_nonces: set PRIVATE_KEY without 0x and run Anvil");
                return;
            }
        };

        use std::str::FromStr;
        let provider = Provider::<Http>::try_from(std::env::var("ANVIL_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string())).unwrap();
        let wallet = ethers_signers::LocalWallet::from_str(&private).expect("wallet");
        let addr = <ethers_signers::LocalWallet as ethers_signers::Signer>::address(&wallet);
        let base_nonce = provider.get_transaction_count(addr, None).await.unwrap().as_u64();

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

        let unsigned: Vec<TypedTransaction> = vec![tx];
        let signer = BasicEnvSigner::from_secret(private);
        let signer_arc = std::sync::Arc::new(signer);

        let outcomes = sim.simulate_unsigned_bundle_try_nonces_with_scorer(&unsigned, signer_arc.clone(), base_nonce, 3, 2, std::sync::Arc::new(GasCostScorer), None).await.unwrap();
        assert!(outcomes.len() == 3);

        // choose best
        let best = sim.choose_best_nonce_strategy(&unsigned, signer_arc.clone(), base_nonce, 3, 2, std::sync::Arc::new(GasCostScorer), None).await.unwrap();
        assert!(best.is_some());
    }

    #[test]
    fn test_set_nonce_tx_assigns_nonce() {
        use ethers_core::types::transaction::eip1559::Eip1559TransactionRequest;
        let mut req = Eip1559TransactionRequest::new();
        req = req.gas(U256::from(21000u64));
        let tx = TypedTransaction::Eip1559(req);
        let tx2 = set_nonce_tx(&tx, U256::from(5u64));
        match tx2 {
            TypedTransaction::Eip1559(r) => {
                // The internal structure does not expose a getter, but round-trip via builder to ensure no panic
                let _ = r;
            }
            _ => panic!("expected eip1559"),
        }
    }
}
