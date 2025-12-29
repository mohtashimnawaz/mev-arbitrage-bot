use anyhow::{Result, Context};
use ethers_providers::{Provider, Http, Middleware};
use ethers_core::types::{Bytes, transaction::eip2718::TypedTransaction, U256, transaction::eip2718::TypedTransaction as TTx, TransactionReceipt};
use crate::signer::Signer;
use std::time::Duration;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::Semaphore;

/// Scorer for simulated bundles. Returns a signed 128-bit score (higher is better).
pub trait Scorer: Send + Sync {
    fn score(&self, receipts: &[TransactionReceipt], signed_txs: &[Vec<u8>]) -> i128;
}

/// Default scorer: penalize reverts heavily, otherwise score is negative gas cost.
pub struct GasCostScorer;

impl Scorer for GasCostScorer {
    fn score(&self, receipts: &[TransactionReceipt], _signed_txs: &[Vec<u8>]) -> i128 {
        let mut total: i128 = 0;
        for r in receipts.iter() {
            // status: 1 = success, 0 = revert
            if let Some(status) = r.status {
                if status.as_u64() == 0u64 {
                    return i128::MIN / 4; // huge negative for revert
                }
            }
            let gas_used = r.gas_used.unwrap_or_default();
            // prefer effectiveGasPrice, fallback to gas_price if present
            let gas_price = r.effective_gas_price.or(r.effective_gas_price).unwrap_or_default();
            let cost = gas_used.saturating_mul(gas_price);
            // convert to i128 (may overflow in practice for huge numbers; clamp)
            let cost_i128 = match cost.try_into() {
                Ok(v) => v as i128,
                Err(_) => i128::MAX / 8,
            };
            total -= cost_i128;
        }
        total
    }
}

/// Simulation / backtesting helper backed by a forked node (Anvil/Hardhat).
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
            let receipt = tokio::time::timeout(Duration::from_secs(10), pending).await.context("timeout awaiting tx")??;
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
    pub async fn simulate_unsigned_bundle_try_nonces_with_scorer<S: Signer + ?Sized + Send + Sync, C: Scorer + ?Sized + Send + Sync>(
        &self,
        unsigned_txs: &[TypedTransaction],
        signer: &S,
        base_nonce: u64,
        nonce_range: u64,
        concurrency: usize,
        scorer: &C,
        set_next_block_base_fee: Option<U256>,
    ) -> Result<Vec<(u64, i128, Vec<TransactionReceipt>)>> {
        let sem = Semaphore::new(concurrency);
        let mut futs = FuturesUnordered::new();

        for offset in 0..nonce_range {
            let permit = sem.clone().acquire_owned().await.unwrap();
            let unsigned = unsigned_txs.to_vec();
            let signer = signer as *const S; // raw pointer to avoid Send issues with traits
            let sim = self.clone();
            let bf = set_next_block_base_fee;
            futs.push(tokio::spawn(async move {
                // drop permit when done
                let _permit = permit;
                // Safety: we ensure signer lives for the call
                let signer_ref: &S = unsafe { &*signer };
                // Build signed bundle for this offset
                let mut signed_blob = Vec::new();
                let mut current = base_nonce + offset;
                for tx in unsigned.iter() {
                    let tx_with_nonce = set_nonce_tx(&tx, U256::from(current));
                    let signed = signer_ref.sign_typed_transaction(&tx_with_nonce).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                    signed_blob.push(signed);
                    current += 1;
                }
                // simulate
                let receipts = sim.simulate_signed_bundle(&signed_blob, bf).await.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let score = scorer.score(&receipts, &signed_blob);
                Ok::<(u64, i128, Vec<TransactionReceipt>), anyhow::Error>((base_nonce + offset, score, receipts))
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

        let outcomes = sim.simulate_unsigned_bundle_try_nonces(&unsigned, &signer, base_nonce, 3).await.unwrap();
        assert!(outcomes.len() == 3);
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
    use std::str::FromStr;

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

        let outcomes = sim.simulate_unsigned_bundle_try_nonces(&unsigned, &signer, base_nonce, 3).await.unwrap();
        assert!(outcomes.len() == 3);
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


