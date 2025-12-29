use async_trait::async_trait;
use anyhow::Result;

/// Signing abstraction. In prod, implement HSM/KMS-backed signer.
#[async_trait]
pub trait Signer: Send + Sync {
    async fn sign_transaction(&self, tx_bytes: &[u8]) -> Result<Vec<u8>>;
}

/// In-memory/test signer (stub)
pub struct InMemorySigner {}

#[async_trait]
impl Signer for InMemorySigner {
    async fn sign_transaction(&self, _tx_bytes: &[u8]) -> Result<Vec<u8>> {
        // TODO: sign using a test key (do NOT use in production)
        Ok(vec![])
    }
}
