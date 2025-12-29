use anyhow::{Result, Context};
use async_trait::async_trait;
use ethers_core::types::transaction::eip2718::TypedTransaction;

/// Signing abstraction. In prod, implement HSM/KMS-backed signer.
#[async_trait]
pub trait Signer: Send + Sync {
    /// Sign arbitrary bytes (legacy helper)
    async fn sign_transaction(&self, tx_bytes: &[u8]) -> Result<Vec<u8>>;

    /// Sign a `TypedTransaction` (EIP-1559 aware) and return signed raw tx bytes.
    async fn sign_typed_transaction(&self, tx: &TypedTransaction) -> Result<Vec<u8>>;
}

/// In-memory/test signer (does nothing; for unit tests)
pub struct InMemorySigner {}

#[async_trait]
impl Signer for InMemorySigner {
    async fn sign_transaction(&self, _tx_bytes: &[u8]) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    async fn sign_typed_transaction(&self, _tx: &TypedTransaction) -> Result<Vec<u8>> {
        // Not implemented for test stub
        Ok(vec![])
    }
}

/// Basic signer that uses `PRIVATE_KEY` environment variable with `ethers-signers`.
/// This is for development only — do NOT use in production. In production, use a
/// hardware signer or remote KMS.
pub struct BasicEnvSigner {
    secret: String,
}

impl BasicEnvSigner {
    pub fn from_env() -> Option<Self> {
        std::env::var("PRIVATE_KEY").ok().map(|s| Self { secret: s })
    }

    /// For tests, allow constructing from a supplied secret.
    #[allow(dead_code)]
    pub fn from_secret(secret: String) -> Self {
        Self { secret }
    }
}

#[async_trait]
impl Signer for BasicEnvSigner {
    async fn sign_transaction(&self, tx_bytes: &[u8]) -> Result<Vec<u8>> {
        use ethers_signers::{LocalWallet, Signer};
        use std::str::FromStr;

        // Parse private key from environment and sign the bytes as a message.
        let wallet = LocalWallet::from_str(&self.secret).context("invalid private key")?;
        let sig = wallet.sign_message(tx_bytes).await.context("failed to sign message")?;
        Ok(sig.to_vec())
    }

    async fn sign_typed_transaction(&self, tx: &TypedTransaction) -> Result<Vec<u8>> {
        use ethers_signers::{LocalWallet, Signer};
        use std::str::FromStr;

        let wallet = LocalWallet::from_str(&self.secret).context("invalid private key")?;
        // Sign transaction and obtain signature
        let sig = wallet.sign_transaction(&tx).await.context("failed to sign tx")?;
        // Attempt to produce RLP of signed tx
        let raw = tx.rlp_signed(&sig);
        Ok(raw.to_vec())
    }
}

/// Remote signer interface (HSM/KMS). Implement this for a client that talks to
/// remote hardware or KMS (over secure channel). We provide a mock for tests.
#[async_trait]
pub trait RemoteSigner: Send + Sync {
    /// Sign a digest (32 bytes) and return serialized signature bytes.
    async fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>>;
}

/// A signer backed by a `RemoteSigner` (HSM/KMS). It will request the remote
/// device to sign the transaction digest and then construct a full signed tx.
pub struct RemoteBasedSigner<R: RemoteSigner + 'static> {
    client: std::sync::Arc<R>,
}

impl<R: RemoteSigner + 'static> RemoteBasedSigner<R> {
    pub fn new(client: std::sync::Arc<R>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl<R: RemoteSigner + 'static> Signer for RemoteBasedSigner<R> {
    async fn sign_transaction(&self, tx_bytes: &[u8]) -> Result<Vec<u8>> {
        // Delegate to remote signer after hashing the payload (HSMs often sign digests)
        let digest = ethers_core::utils::keccak256(tx_bytes);
        let sig = self.client.sign_digest(&digest).await.context("remote sign failed")?;
        Ok(sig)
    }

    async fn sign_typed_transaction(&self, tx: &TypedTransaction) -> Result<Vec<u8>> {
        // Compute sighash and ask remote to sign it
        let sighash = tx.sighash();
        let sig = self.client.sign_digest(sighash.as_bytes()).await.context("remote sign failed")?;
        // NOTE: we assume remote returns a recoverable signature compatible with ethers' Signature
        // For tests with MockRemoteSigner we will return a valid RLP using local codepath instead.
        Err(anyhow::anyhow!("rlp signing via remote is not implemented in generic RemoteBasedSigner"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::build_eip1559_tx;
    use ethers_core::types::{U256, Address, Bytes};

    #[tokio::test]
    async fn basic_env_signer_signs_typed_tx() {
        // Use a deterministic test private key (do NOT use for real funds)
        // Private key without 0x prefix (LocalWallet::from_str expects hex without 0x)
        let secret = "0123456789012345678901234567890123456789012345678901234567890123".to_string();
        let signer = BasicEnvSigner::from_secret(secret);

        let tx = build_eip1559_tx(
            U256::from(0u64),
            Address::zero(),
            U256::from(0u64),
            Bytes::from(vec![]),
            U256::from(21000u64),
            U256::from(1_000_000_000u64),
            U256::from(100_000_000_000u64),
            1u64,
        );

        let raw = signer.sign_typed_transaction(&tx).await.unwrap();
        assert!(raw.len() > 0);
    }

    struct MockRemote {
        // For the mock, we'll sign using a local wallet under the hood
        secret: String,
    }

    #[async_trait]
    impl RemoteSigner for MockRemote {
        async fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>> {
            use ethers_signers::LocalWallet;
            use std::str::FromStr;
            let wallet = LocalWallet::from_str(&self.secret).context("invalid private key")?;
            // sign_hash is synchronous for LocalWallet (returns Signature)
            let sig = wallet.sign_hash(ethers_core::types::H256::from_slice(digest));
            Ok(sig.to_vec())
        }
    }

    #[tokio::test]
    async fn remote_based_signer_delegates_and_returns_sig() {
        let secret = "0123456789012345678901234567890123456789012345678901234567890123".to_string();
        let mock = std::sync::Arc::new(MockRemote { secret });
        let s = RemoteBasedSigner::new(mock);
        let b = s.sign_transaction(b"hello").await.unwrap();
        assert!(b.len() > 0);
    }
}
