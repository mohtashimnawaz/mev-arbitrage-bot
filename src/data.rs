use anyhow::{Result, Context};
use serde::Serialize;
use std::time::Duration;
use tokio::sync::broadcast;
use ethers_providers::{Provider, Http, Middleware};
use tokio_tungstenite::connect_async;
use futures_util::{StreamExt, SinkExt};
use serde_json::json;

/// Simple normalized quote
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub pair: String,
    pub price: f64,
    pub timestamp_ms: u128,
}

/// Market data client that publishes `Quote` messages on a broadcast channel.
/// This implementation supports multiple HTTP RPC providers (polled) and
/// multiple WebSocket endpoints (subscribed). It performs basic health
/// checks and reconnection with exponential backoff.
pub struct MarketDataClient {
    pub sender: broadcast::Sender<Quote>,
    rpc_urls: Vec<String>,
    ws_urls: Vec<String>,
}

impl MarketDataClient {
    pub async fn new(rpc_urls: Vec<String>, ws_urls: Vec<String>) -> Result<Self> {
        let (sender, _recv) = broadcast::channel(2048);
        Ok(Self { sender, rpc_urls, ws_urls })
    }

    pub async fn start(&self) -> Result<()> {
        let tx = self.sender.clone();

        // If no providers configured, fall back to synthetic generator
        if self.rpc_urls.is_empty() && self.ws_urls.is_empty() {
            tokio::spawn(async move {
                loop {
                    let q = Quote {
                        pair: "ETH/USDC".to_string(),
                        price: 1200.0 + (rand::random::<f64>() * 10.0 - 5.0),
                        timestamp_ms: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis(),
                    };
                    let _ = tx.send(q);
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            });
            return Ok(());
        }

        // Spawn HTTP RPC pollers
        for url in self.rpc_urls.clone() {
            let tx = self.sender.clone();
            tokio::spawn(async move {
                // Create provider for this RPC
                let provider = match Provider::<Http>::try_from(url.as_str()) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!(%e, %url, "failed to create HTTP provider");
                        return;
                    }
                };

                let mut last_bn: Option<u64> = None;
                let mut backoff = 100u64; // ms
                loop {
                    match provider.get_block_number().await {
                        Ok(bn) => {
                            let bn_u64 = bn.as_u64();
                            if Some(bn_u64) != last_bn.map(|v| v as u64) {
                                last_bn = Some(bn_u64);
                                // Derive a lightweight pseudo-price from block number for now
                                let price = 1200.0 + ((bn_u64 % 100) as f64) * 0.1;
                                let timestamp_ms = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis();
                                let q = Quote { pair: "ETH/USDC".to_string(), price, timestamp_ms };
                                let _ = tx.send(q);
                            }
                            backoff = 100;
                        }
                        Err(e) => {
                            tracing::warn!(%e, %url, "rpc poll error, backing off");
                            tokio::time::sleep(Duration::from_millis(backoff)).await;
                            backoff = (backoff * 2).min(10_000);
                            continue;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            });
        }

        // Spawn WebSocket subscribers
        for url in self.ws_urls.clone() {
            let tx = self.sender.clone();
            tokio::spawn(async move {
                let mut backoff = 100u64;
                loop {
                    match connect_async(url.as_str()).await {
                        Ok((mut ws_stream, _resp)) => {
                            tracing::info!(%url, "ws connected");
                            // Subscribe to new heads
                            let sub = json!({"jsonrpc":"2.0","id":1,"method":"eth_subscribe","params":["newHeads"]});
                            if ws_stream.send(tokio_tungstenite::tungstenite::Message::Text(sub.to_string())).await.is_err() {
                                tracing::warn!(%url, "ws send subscribe failed");
                                continue;
                            }

                            backoff = 100;
                            while let Some(msg) = ws_stream.next().await {
                                match msg {
                                    Ok(tokio_tungstenite::tungstenite::Message::Text(txt)) => {
                                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                                            // When new head notifications arrive, extract block number if present
                                            if let Some(params) = v.get("params") {
                                                if let Some(result) = params.get("result") {
                                                    if let Some(number) = result.get("number") {
                                                        if let Some(number_str) = number.as_str() {
                                                            if let Ok(bn) = u64::from_str_radix(number_str.trim_start_matches("0x"), 16) {
                                                                let price = 1200.0 + ((bn % 100) as f64) * 0.1;
                                                                let timestamp_ms = std::time::SystemTime::now()
                                                                    .duration_since(std::time::UNIX_EPOCH)
                                                                    .unwrap()
                                                                    .as_millis();
                                                                let q = Quote { pair: "ETH/USDC".to_string(), price, timestamp_ms };
                                                                let _ = tx.send(q);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        tracing::warn!(%e, %url, "ws recv error");
                                        break;
                                    }
                                }
                            }
                            tracing::info!(%url, "ws disconnected, will reconnect");
                        }
                        Err(e) => {
                            tracing::warn!(%e, %url, "ws connect failed, backing off");
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                    backoff = (backoff * 2).min(10_000);
                }
            });
        }

        Ok(())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Quote> {
        self.sender.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn it_publishes_quotes_without_providers() {
        let client = MarketDataClient::new(vec![], vec![]).await.unwrap();
        client.start().await.unwrap();
        let mut rx = client.subscribe();
        let q = rx.recv().await.unwrap();
        assert!(q.price > 0.0);
    }
}
