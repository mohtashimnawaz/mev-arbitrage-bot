use anyhow::{Result, anyhow};
use crate::kms::KmsClient;

/// YubiHSM adapter skeleton. Implement the real YubiHSM client integration as needed.
pub struct YubiHsm {
    pub connector: String,
}

impl YubiHsm {
    pub fn new(connector: String) -> Self {
        Self { connector }
    }
}

#[async_trait::async_trait]
impl KmsClient for YubiHsm {
    async fn sign(&self, _digest: &[u8]) -> Result<Vec<u8>> {
        Err(anyhow!("YubiHsm adapter not implemented: provide a concrete implementation for your HSM environment"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn yubihsm_skeleton_returns_err() {
        let y = YubiHsm::new("tcp://127.0.0.1:12345".to_string());
        assert!(y.sign(&[0u8; 32]).await.is_err());
    }
}
