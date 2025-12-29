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
        let sig_bytes = self.client.sign_digest(sighash.as_bytes()).await.context("remote sign failed")?;

        // Attempt to parse as DER signature first (common for KMS). If that fails,
        // accept compact (r||s||v) or r||s with v appended.
        use crate::crypto::der::der_to_ethers_signature;
        let maybe_sig = der_to_ethers_signature(&sig_bytes, sighash.as_bytes(), None);
        let ethers_sig = match maybe_sig {
            Ok(s) => s,
            Err(_) => {
                // Try compact form: 65 bytes (r||s||v)
                if sig_bytes.len() == 65 {
                    let r = ethers_core::types::U256::from_big_endian(&sig_bytes[0..32]);
                    let s = ethers_core::types::U256::from_big_endian(&sig_bytes[32..64]);
                    let v = sig_bytes[64] as u64;
                    ethers_core::types::Signature { r, s, v }
                } else if sig_bytes.len() == 64 {
                    // no v provided; attempt recovery by trying recid 0..3 using k256
                    use k256::ecdsa::Signature as KSignature;
                    use secp256k1::{Secp256k1, ecdsa::{RecoverableSignature, RecoveryId}};
                    let compact = &sig_bytes[..];
                    // k256 requires GenericArray; use secp to recover
                    let secp = Secp256k1::new();
                    let msg = secp256k1::Message::from_slice(sighash.as_bytes()).map_err(|e| anyhow::anyhow!(e))?;
                    let mut found: Option<ethers_core::types::Signature> = None;
                    for recid_val in 0..4 {
                        let recid = RecoveryId::from_i32(recid_val).map_err(|e| anyhow::anyhow!(e))?;
                        if let Ok(rec_sig) = RecoverableSignature::from_compact(compact, recid) {
                            if let Ok(pk) = secp.recover_ecdsa(&msg, &rec_sig) {
                                let serialized = pk.serialize_uncompressed();
                                let pubkey_bytes = &serialized[1..65];
                                let _addr_bytes = ethers_core::utils::keccak256(pubkey_bytes);
                                // accept first recovered sig
                                let r = ethers_core::types::U256::from_big_endian(&compact[0..32]);
                                let s = ethers_core::types::U256::from_big_endian(&compact[32..64]);
                                let v = (recid_val as u64) + 27u64;
                                found = Some(ethers_core::types::Signature { r, s, v });
                                break;
                            }
                        }
                    }
                    found.ok_or_else(|| anyhow::anyhow!("could not recover signature"))?
                } else {
                    return Err(anyhow::anyhow!("unsupported signature format from remote"));
                }
            }
        };

        // Normalize `v` for typed transactions (EIP-1559 expects parity 0/1)
        let normalized_sig = match tx {
            TypedTransaction::Eip1559(_) => {
                let v_parity = if ethers_sig.v >= 27 { ethers_sig.v - 27 } else { ethers_sig.v };
                ethers_core::types::Signature { r: ethers_sig.r, s: ethers_sig.s, v: v_parity }
            }
            _ => ethers_sig,
        };

        // RLP sign the transaction using ethers helper
        let raw = tx.rlp_signed(&normalized_sig);
        Ok(raw.to_vec())
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
        // For the mock, we'll sign using secp256k1 to return DER-encoded signatures (like KMS)
        secret: String,
    }

    #[async_trait]
    impl RemoteSigner for MockRemote {
        async fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>> {
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
    }

    #[tokio::test]
    async fn remote_based_signer_delegates_and_returns_sig() {
        let secret = "0123456789012345678901234567890123456789012345678901234567890123".to_string();
        let mock = std::sync::Arc::new(MockRemote { secret });
        let s = RemoteBasedSigner::new(mock);

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

        let raw = s.sign_typed_transaction(&tx).await.unwrap();
        assert!(raw.len() > 0);
    }
}
