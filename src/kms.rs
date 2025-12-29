use anyhow::{Result, Context};
use ethers_core::types::Address;

/// KMS client trait for signing digests (abstracts AWS KMS, YubiHSM, etc.)
#[async_trait::async_trait]
pub trait KmsClient: Send + Sync + 'static {
    /// Sign a 32-byte digest and return signature bytes in a compatible format
    async fn sign(&self, digest: &[u8]) -> Result<Vec<u8>>;

    /// Optional: return public key or address associated with the key (useful for verifying recoverable signatures)
    async fn get_address(&self) -> Result<Option<Address>> { Ok(None) }

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
        // Use k256/secp256k1 to produce a DER-encoded ECDSA signature that mimics KMS.
        use secp256k1::{Secp256k1, SecretKey, Message as SecpMessage};
        // secret is raw hex of private key
        let sk_bytes = hex::decode(&self.secret).map_err(|e| anyhow::anyhow!(e))?;
        let sk = SecretKey::from_slice(&sk_bytes).map_err(|e| anyhow::anyhow!(e))?;
        let secp = Secp256k1::new();
        let msg = SecpMessage::from_slice(digest).map_err(|e| anyhow::anyhow!(e))?;
        let recsig = secp.sign_ecdsa_recoverable(&msg, &sk);
        let std = recsig.to_standard();
        Ok(std.serialize_der().to_vec())
    }

    async fn get_address(&self) -> Result<Option<Address>> {
        use ethers_signers::LocalWallet;
        use ethers_signers::Signer as _; // bring the trait into scope for `address()`
        use std::str::FromStr;
        let wallet = LocalWallet::from_str(&self.secret).context("invalid private key")?;
        Ok(Some(wallet.address()))
    }
}

pub mod aws;
pub mod yubihsm;

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
