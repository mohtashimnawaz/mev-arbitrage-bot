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

        /// Sign a 32-byte digest using the KMS Sign API with ECDSA_SHA_256 algorithm.
        pub async fn sign_digest(&self, digest: &[u8]) -> Result<Vec<u8>> {
            if digest.len() != 32 {
                return Err(anyhow!("KMS sign expects 32-byte digest"));
            }
            let resp = self.client.sign()
                .key_id(self.key_id.clone())
                .message(digest.into())
                .message_type(kms::model::MessageType::Digest)
                .signing_algorithm(SigningAlgorithmSpec::EcdsaSha256)
                .send()
                .await?;
            if let Some(sig) = resp.signature() {
                // AWS returns DER-encoded ASN.1 signature for ECDSA
                Ok(sig.as_ref().to_vec())
            } else {
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
