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

    /// Create a client with an explicit relay URL (useful for tests to avoid
    /// modifying the global environment and causing test interference).
    pub fn with_url(relay_url: String) -> Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
        Ok(Self { client, relay_url: Some(relay_url) })
    }

    /// Create a client with no relay configured (useful for testing fallback behaviour).
    pub fn without_relay() -> Result<Self> {
        let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
        Ok(Self { client, relay_url: None })
    }

    /// Submit a generic bundle body (legacy compatibility).
    pub async fn submit_bundle(&self, bundle: &[u8]) -> Result<String> {
        if let Some(url) = &self.relay_url {
            let body = base64::encode(bundle);
            let resp = self.client.post(url)
                .json(&serde_json::json!({"bundle": body}))
                .send().await.context("relay post failed")?;
            let txt = resp.text().await.unwrap_or_default();
            Ok(txt)
        } else {
            tracing::info!("No relay configured; bundle size {} bytes", bundle.len());
            Ok("stub".to_string())
        }
    }

    /// Submit a Flashbots-style bundle (array of signed raw tx hex strings).
    /// `signed_txs` is a slice of raw signed tx bytes.
    /// `block_number` is optional target block number; if None, relay decides.
    pub async fn submit_flashbots_bundle(&self, signed_txs: &[Vec<u8>], block_number: Option<u64>) -> Result<serde_json::Value> {
        let url = match &self.relay_url {
            Some(u) => u.clone(),
            None => return Err(anyhow::anyhow!("FLASHBOTS_RELAY_URL not configured")),
        };

        let txs: Vec<String> = signed_txs.iter().map(|s| format!("0x{}", hex::encode(s))).collect();
        let mut params = serde_json::Map::new();
        params.insert("txs".to_string(), serde_json::Value::Array(txs.into_iter().map(serde_json::Value::String).collect()));
        if let Some(bn) = block_number {
            params.insert("blockNumber".to_string(), serde_json::Value::String(format!("0x{:x}", bn)));
        }

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_sendBundle",
            "params": [params]
        });

        let resp = self.client.post(&url)
            .json(&req)
            .send().await.context("flashbots post failed")?;
        let v = resp.json::<serde_json::Value>().await.context("invalid json response from relay")?;
        Ok(v)
    }

    /// Call the relay's simulate endpoint to get relay-side simulation semantics.
    pub async fn simulate_flashbots_bundle(&self, signed_txs: &[Vec<u8>], block_number: Option<u64>) -> Result<serde_json::Value> {
        let url = match &self.relay_url {
            Some(u) => u.clone(),
            None => return Err(anyhow::anyhow!("FLASHBOTS_RELAY_URL not configured")),
        };
        let txs: Vec<String> = signed_txs.iter().map(|s| format!("0x{}", hex::encode(s))).collect();
        let mut params = serde_json::Map::new();
        params.insert("txs".to_string(), serde_json::Value::Array(txs.into_iter().map(serde_json::Value::String).collect()));
        if let Some(bn) = block_number {
            params.insert("blockNumber".to_string(), serde_json::Value::String(format!("0x{:x}", bn)));
        }
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_simulateBundle",
            "params": [params]
        });
        let resp = self.client.post(&url).json(&req).send().await.context("flashbots simulate failed")?;
        let v = resp.json::<serde_json::Value>().await.context("invalid json response from relay simulate")?;
        Ok(v)
    }
}

#[cfg(test)]
mod simulate_tests {
    use super::*;

    #[tokio::test]
    async fn simulate_flashbots_bundle_posts_to_relay() {
        let server = httpmock::MockServer::start();
        let m = server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/");
            then.status(200).body(r#"{"result":"sim_ok"}"#);
        });

        let rc = RelayClient::with_url(server.url("/")).unwrap();
        let signed = vec![vec![0x01,0x02,0x03]];
        let v = rc.simulate_flashbots_bundle(&signed, Some(12345)).await.unwrap();
        assert_eq!(v.get("result").unwrap().as_str().unwrap(), "sim_ok");
        m.assert();
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn relay_submit_bundle_falls_back_when_no_relay() {
        let rc = RelayClient::without_relay().unwrap();
        let res = rc.submit_bundle(&[1u8,2,3]).await.unwrap();
        assert_eq!(res, "stub");
    }

    #[tokio::test]
    async fn submit_flashbots_bundle_posts_to_relay() {
        // start a mock HTTP server
        let server = httpmock::MockServer::start();
        let m = server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/");
            then.status(200).body(r#"{"result":"ok"}"#);
        });

        let rc = RelayClient::with_url(server.url("/")).unwrap();
        let signed = vec![vec![0x01,0x02,0x03]];
        let v = rc.submit_flashbots_bundle(&signed, Some(12345)).await.unwrap();
        assert_eq!(v.get("result").unwrap().as_str().unwrap(), "ok");
        m.assert();
    }
}
