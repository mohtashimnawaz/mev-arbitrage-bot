use anyhow::{Result, Context};
use async_trait::async_trait;

/// Signing abstraction. In prod, implement HSM/KMS-backed signer.
#[async_trait]
pub trait Signer: Send + Sync {
    async fn sign_transaction(&self, tx_bytes: &[u8]) -> Result<Vec<u8>>;
}

/// In-memory/test signer (does nothing; for unit tests)
pub struct InMemorySigner {}

#[async_trait]
impl Signer for InMemorySigner {
    async fn sign_transaction(&self, _tx_bytes: &[u8]) -> Result<Vec<u8>> {
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
}
