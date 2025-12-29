use anyhow::{Result, Context};
use reqwest::Client;
use std::time::Duration;

/// Relay client that can submit bundles to a configured relay endpoint.
pub struct RelayClient {
    client: Client,
    relay_url: Option<String>,
}

impl RelayClient {
    pub async fn new() -> Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
        let relay_url = std::env::var("FLASHBOTS_RELAY_URL").ok();
        Ok(Self { client, relay_url })
    }

    pub async fn submit_bundle(&self, bundle: &[u8]) -> Result<String> {
        if let Some(url) = &self.relay_url {
            let body = base64::encode(bundle);
            let resp = self.client.post(url)
                .json(&serde_json::json!({"bundle": body}))
                .send().await.context("relay post failed")?;
            let txt = resp.text().await.unwrap_or_default();
            Ok(txt)
        } else {
            // No relay configured: log and return stub status
            tracing::info!("No relay configured; bundle size {} bytes", bundle.len());
            Ok("stub".to_string())
        }
    }
}
