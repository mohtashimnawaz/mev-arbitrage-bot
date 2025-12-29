use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub rpc_urls: Vec<String>,
    pub ws_urls: Vec<String>,
    pub profit_threshold_wei: u128,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rpc_urls: vec!["http://localhost:8545".to_string()],
            ws_urls: vec![],
            profit_threshold_wei: 1_000_000_000_000_000, // example: 0.001 ETH
        }
    }
}
