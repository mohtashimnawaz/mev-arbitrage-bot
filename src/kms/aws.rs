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

        /// Parse the SubjectPublicKeyInfo using ASN.1 and ensure the key is EC/secp256k1.
        /// Returns the Ethereum address extracted from the uncompressed public key.
        /// Parse the SubjectPublicKeyInfo using ASN.1 and ensure the key is EC/secp256k1.
        /// Returns the Ethereum address extracted from the uncompressed public key.
        pub fn parse_spki_to_address(pk: &[u8]) -> Result<ethers_core::types::Address> {
            use x509_parser::prelude::*;
            // parse_public_key_info returns (rem, SubjectPublicKeyInfo)
            let (_, spki) = SubjectPublicKeyInfo::from_der(pk).map_err(|e| anyhow!("failed to parse SubjectPublicKeyInfo: {}", e))?;

            // algorithm OID should be id-ecPublicKey (1.2.840.10045.2.1)
            let alg_oid = spki.algorithm.algorithm;
            let oid_str = alg_oid.to_id_string();
            if oid_str != "1.2.840.10045.2.1" {
                return Err(anyhow!("unsupported key algorithm OID: {}", oid_str));
            }
            // parameters must indicate the named curve OID; for secp256k1 it's 1.3.132.0.10
            if let Some(params) = spki.algorithm.parameters.as_ref() {
                if let Ok(oid) = params.as_oid() {
                    let curve_oid = oid.to_id_string();
                    if curve_oid != "1.3.132.0.10" {
                        return Err(anyhow!("unsupported EC curve OID: {} (requires secp256k1)", curve_oid));
                    }
                } else {
                    return Err(anyhow!("unsupported SPKI parameters for EC key"));
                }
            } else {
                return Err(anyhow!("missing SPKI parameters for EC key"));
            }

            // get the public key bitstring (should be uncompressed form 0x04||X||Y)
            let spk = spki.subject_public_key.data;
            if spk.len() != 65 || spk[0] != 0x04 {
                return Err(anyhow!("unexpected EC point format: expected uncompressed 65-byte point"));
            }
            let pub_bytes = &spk[1..65];
            let addr_bytes = ethers_core::utils::keccak256(pub_bytes);
            let addr = ethers_core::types::Address::from_slice(&addr_bytes[12..]);
            Ok(addr)
        }

        pub async fn get_address(&self) -> Result<Option<ethers_core::types::Address>> {
            let pk = self.get_public_key().await?;
            // Strict parse; if it fails, return the error (do not fallback to heuristics)
            match Self::parse_spki_to_address(&pk) {
                Ok(a) => Ok(Some(a)),
                Err(e) => Err(e),
            }
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
