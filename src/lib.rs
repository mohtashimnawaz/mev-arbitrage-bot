pub mod config;
pub mod data;
pub mod executor;
pub mod scanner;
pub mod signer;
pub mod sim;

use anyhow::Result;

pub async fn run() -> Result<()> {
    // TODO: wire components together
    Ok(())
}

pub async fn simulate() -> Result<()> {
    // TODO: simulation entrypoint
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_stub() {
        run().await.unwrap();
    }
}
