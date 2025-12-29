use anyhow::{Result, anyhow};
use crate::kms::KmsClient;

/// AWS KMS adapter skeleton. When compiled with the `aws-kms` feature, this provides
/// a thin wrapper around the AWS SDK for KMS. Note: AWS KMS returns DER-encoded
/// ECDSA signatures; additional steps are required to obtain Ethereum-compatible
/// recoverable signatures (v) — callers should be aware.
pub struct AwsKms {
    pub key_id: String,
}

impl AwsKms {
    pub fn new(key_id: String) -> Self {
        Self { key_id }
    }
}

#[cfg(feature = "aws-kms")]
mod real {
    use super::*;
    use aws_sdk_kms as kms;
    use aws_config;
    use kms::model::SigningAlgorithmSpec;

    pub struct AwsKmsClient {
        key_id: String,
        client: kms::Client,
    }

    impl AwsKmsClient {
        pub async fn from_env(key_id: String) -> Result<Self> {
            let config = aws_config::load_from_env().await;
            let client = kms::Client::new(&config);
            Ok(Self { key_id, client })
        }

        /// Returns the raw public key bytes (DER encoded) for the configured key.
        pub async fn get_public_key(&self) -> Result<Vec<u8>> {
            let resp = self.client.get_public_key().key_id(self.key_id.clone()).send().await?;
            if let Some(pk) = resp.public_key() {
                Ok(pk.as_ref().to_vec())
            } else {
                Err(anyhow!("no public key available for key"))
            }
        }

        /// Attempt to extract an Ethereum address from the DER-encoded SubjectPublicKeyInfo returned
        /// by `GetPublicKey`. This is a heuristic that looks for an uncompressed EC point (0x04 || X || Y)
        /// inside the returned bytes and converts it to an address.
        pub async fn get_address(&self) -> Result<Option<ethers_core::types::Address>> {
            let pk = self.get_public_key().await?;
            // Search for a 65-byte uncompressed point starting with 0x04
            for i in 0..(pk.len().saturating_sub(65)) {
                if pk[i] == 0x04 {
                    let slice = &pk[i..i+65];
                    let pub_bytes = &slice[1..65];
                    let addr_bytes = ethers_core::utils::keccak256(pub_bytes);
                    let addr = ethers_core::types::Address::from_slice(&addr_bytes[12..]);
                    return Ok(Some(addr));
                }
            }
            Ok(None)
        }

        /// Sign a 32-byte digest using the KMS Sign API with ECDSA_SHA_256 algorithm.
        pub async fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>> {
            if digest.len() != 32 {
                return Err(anyhow!("KMS sign expects 32-byte digest"));
            }
            #[cfg(feature = "with-metrics")]
            {
                metrics::increment_counter!("kms.sign.attempts", 1);
            }
            let resp = self.client.sign()
                .key_id(self.key_id.clone())
                .message(digest.into())
                .message_type(kms::model::MessageType::Digest)
                .signing_algorithm(SigningAlgorithmSpec::EcdsaSha256)
                .send()
                .await?;
            if let Some(sig) = resp.signature() {
                #[cfg(feature = "with-metrics")]
                {
                    metrics::increment_counter!("kms.sign.success", 1);
                }
                // AWS returns DER-encoded ASN.1 signature for ECDSA
                Ok(sig.as_ref().to_vec())
            } else {
                #[cfg(feature = "with-metrics")]
                {
                    metrics::increment_counter!("kms.sign.failure", 1);
                }
                Err(anyhow!("no signature returned from KMS"))
            }
        }
    }

    #[async_trait::async_trait]
    impl KmsClient for AwsKmsClient {
        async fn sign(&self, digest: &[u8]) -> Result<Vec<u8>> {
            self.sign_digest(digest).await
        }
    }
}

#[async_trait::async_trait]
impl KmsClient for AwsKms {
    async fn sign(&self, _digest: &[u8]) -> Result<Vec<u8>> {
        Err(anyhow!("AwsKms adapter not implemented: enable 'aws-kms' feature to use the real client"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn aws_kms_skeleton_returns_err() {
        let a = AwsKms::new("dummy".to_string());
        let res = a.sign(&[0u8; 32]).await;
        assert!(res.is_err());
    }
}
