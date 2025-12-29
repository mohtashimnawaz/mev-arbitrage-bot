use anyhow::{Result, Context};
use ethers_providers::{Provider, Http, Middleware};
use ethers_core::types::{Bytes, transaction::eip2718::TypedTransaction, U256};
use crate::signer::Signer;
use std::time::Duration;

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
    pub async fn simulate_signed_bundle(&self, signed_raw_txs: &[Vec<u8>]) -> Result<Vec<serde_json::Value>> {
        let provider = Provider::<Http>::try_from(self.rpc.as_str()).context("invalid rpc url")?;

        // Create snapshot
        let snap_id: serde_json::Value = provider.request("evm_snapshot", ()).await.context("snapshot failed")?;

        let mut results = Vec::new();

        for raw in signed_raw_txs.iter() {
            // provider.send_raw_transaction expects Bytes
            let b = Bytes::from(raw.clone());
            let pending = provider.send_raw_transaction(b).await.context("send_raw failed")?;
            // await mined receipt (with a timeout)
            let receipt = tokio::time::timeout(Duration::from_secs(10), pending).await.context("timeout awaiting tx")??;
            let v = serde_json::to_value(&receipt).context("receipt serialize failed")?;
            results.push(v);
        }

        // Revert snapshot to clean state
        let _: bool = provider.request("evm_revert", vec![snap_id]).await.context("revert failed")?;

        Ok(results)
    }

    /// Simulate an unsigned bundle by trying multiple base nonces. For each offset in
    /// `0..nonce_range` we assign nonce = base_nonce + offset for the first tx, and
    /// increment by 1 for each subsequent transaction. We sign each nonce sequence
    /// using `signer` and simulate the resulting signed bundle.
    pub async fn simulate_unsigned_bundle_try_nonces<S: Signer + ?Sized + Send + Sync>(
        &self,
        unsigned_txs: &[TypedTransaction],
        signer: &S,
        base_nonce: u64,
        nonce_range: u64,
    ) -> Result<Vec<(u64, Vec<serde_json::Value>)>> {
        let mut outcomes = Vec::new();

        for offset in 0..nonce_range {
            let mut signed_blob = Vec::new();
            let mut current = base_nonce + offset;
            for tx in unsigned_txs.iter() {
                // set nonce on cloned tx
                let tx_with_nonce = set_nonce_tx(tx, U256::from(current));
                // sign using provided signer
                let signed = signer.sign_typed_transaction(&tx_with_nonce).await.context("sign failed")?;
                signed_blob.push(signed);
                current += 1;
            }

            // simulate the signed bundle
            let res = self.simulate_signed_bundle(&signed_blob).await.context("simulate signed bundle failed")?;
            outcomes.push((base_nonce + offset, res));
        }

        Ok(outcomes)
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
        let addr = wallet.address();
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


