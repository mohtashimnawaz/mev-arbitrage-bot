use anyhow::Result;

/// KMS client trait for signing digests (abstracts AWS KMS, YubiHSM, etc.)
#[async_trait::async_trait]
pub trait KmsClient: Send + Sync + 'static {
    /// Sign a 32-byte digest and return signature bytes in a compatible format
    async fn sign(&self, digest: &[u8]) -> Result<Vec<u8>>;
}

/// Mock KMS client for tests
pub struct MockKms {
    secret: String,
}

impl MockKms {
    pub fn new(secret: String) -> Self {
        Self { secret }
    }
}

#[async_trait::async_trait]
impl KmsClient for MockKms {
    async fn sign(&self, digest: &[u8]) -> Result<Vec<u8>> {
        use ethers_signers::{LocalWallet, Signer};
        use std::str::FromStr;
        let wallet = LocalWallet::from_str(&self.secret)?;
        // Sign the raw digest (H256)
        let sig = wallet.sign_hash(ethers_core::types::H256::from_slice(digest))?;
        Ok(sig.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_kms_signs_digest() {
        let secret = "0123456789012345678901234567890123456789012345678901234567890123".to_string();
        let m = MockKms::new(secret);
        let digest = [1u8; 32];
        let s = m.sign(&digest).await.unwrap();
        assert!(s.len() > 0);
    }
}
