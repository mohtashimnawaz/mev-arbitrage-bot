use anyhow::Result;
use serde::Serialize;
use std::time::Duration;
use tokio::sync::broadcast;

/// Simple normalized quote
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub pair: String,
    pub price: f64,
    pub timestamp_ms: u128,
}

/// Market data client that publishes `Quote` messages on a broadcast channel.
/// This is a minimal implementation that periodically emits synthetic quotes.
pub struct MarketDataClient {
    pub sender: broadcast::Sender<Quote>,
}

impl MarketDataClient {
    pub async fn new(_urls: Vec<String>) -> Result<Self> {
        let (sender, _recv) = broadcast::channel(1024);
        Ok(Self { sender })
    }

    pub async fn start(&self) -> Result<()> {
        let tx = self.sender.clone();
        // spawn a background task that emits synthetic quotes; replace with real WS/RPC listeners
        tokio::spawn(async move {
            loop {
                let q = Quote {
                    pair: "ETH/USDC".to_string(),
                    price: 1200.0 + (rand::random::<f64>() * 10.0 - 5.0),
                    timestamp_ms: (tokio::time::Instant::now().elapsed().as_millis()),
                };
                let _ = tx.send(q);
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });
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
    async fn it_publishes_quotes() {
        let client = MarketDataClient::new(vec![]).await.unwrap();
        client.start().await.unwrap();
        let mut rx = client.subscribe();
        let q = rx.recv().await.unwrap();
        assert!(q.price > 0.0);
    }
}
