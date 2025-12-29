use anyhow::{Result, anyhow};
use crate::kms::KmsClient;

/// AWS KMS adapter skeleton. Implement the real AWS SDK integration behind a feature flag
/// in production. This adapter currently serves as a placeholder and will return an error
/// until implemented.
pub struct AwsKms {
    pub key_id: String,
}

impl AwsKms {
    pub fn new(key_id: String) -> Self {
        Self { key_id }
    }
}

#[async_trait::async_trait]
impl KmsClient for AwsKms {
    async fn sign(&self, _digest: &[u8]) -> Result<Vec<u8>> {
        Err(anyhow!("AwsKms adapter not implemented: enable 'aws-kms' feature and implement AWS SDK integration"))
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
