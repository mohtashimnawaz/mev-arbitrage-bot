use anyhow::Result;

/// Scanner stub: detect safe arbitrage & liquidation opportunities.
pub struct Scanner {}

impl Scanner {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn scan(&self) -> Result<Vec<String>> {
        // TODO: implement detection logic and return candidate trades (serialized)
        Ok(vec![])
    }
}
