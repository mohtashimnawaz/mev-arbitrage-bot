use anyhow::Result;

/// Market data client stub: subscribes to WS and polls RPCs; normalizes quotes.
pub struct MarketDataClient {}

impl MarketDataClient {
    pub async fn new(_urls: Vec<String>) -> Result<Self> {
        // TODO: implement multi-provider WS/RPC feed
        Ok(Self {})
    }

    pub async fn start(&self) -> Result<()> {
        // TODO: spawn WS listeners and provide a normalized stream
        Ok(())
    }
}
