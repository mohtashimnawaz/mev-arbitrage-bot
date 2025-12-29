use std::str::FromStr;
use httpmock::MockServer;
use ethers_providers::{Provider, Http, Middleware};
use ethers_core::types::{U256, Address, Bytes};
use mev_arbitrage_bot::tx::build_eip1559_tx;
use mev_arbitrage_bot::signer::{BasicEnvSigner, Signer};
use mev_arbitrage_bot::executor::RelayClient;

// E2E test - ignored by default. Requires env vars:
// - ANVIL_RPC_URL (default: http://127.0.0.1:8545)
// - PRIVATE_KEY (hex without 0x) for a funded Anvil account
// - FLASHBOTS_RELAY_URL will be set to a mock server in the test

#[tokio::test]
#[ignore]
async fn e2e_build_sign_bundle_submit_and_mine() {
    let anvil_rpc = std::env::var("ANVIL_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
    let private = match std::env::var("PRIVATE_KEY") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("Skipping E2E: set PRIVATE_KEY without 0x (Anvil funded key)");
            return;
        }
    };

    // Start a mock relay that will accept the bundle
    let server = MockServer::start();
    let m = server.mock(|when, then| {
        when.method(httpmock::Method::POST).path("/");
        then.status(200).body(r#"{"result":"accepted"}"#);
    });

    // Wire relay URL
    unsafe { std::env::set_var("FLASHBOTS_RELAY_URL", server.url("/")); }

    // Provider
    let provider = Provider::<Http>::try_from(anvil_rpc.as_str()).expect("provider");
    let chain_id = provider.get_chainid().await.expect("chainid").as_u64();

    // Build simple tx to self
    let nonce = provider.get_transaction_count(Address::from_str("0x0000000000000000000000000000000000000000").unwrap(), None).await.expect("nonce");
    let tx = build_eip1559_tx(
        nonce,
        Address::from_str("0x0000000000000000000000000000000000000000").unwrap(),
        U256::from(0u64),
        Bytes::from(vec![]),
        U256::from(21000u64),
        U256::from(1_000_000_000u64),
        U256::from(100_000_000_000u64),
        chain_id,
    );

    // Sign via BasicEnvSigner
    let signer = BasicEnvSigner::from_secret(private);
    let raw = signer.sign_typed_transaction(&tx).await.expect("sign tx");

    // Submit to flashbots mock relay via RelayClient
    let rc = RelayClient::new().await.unwrap();
    let v = rc.submit_flashbots_bundle(&[raw.clone()], None).await.expect("submit bundle");
    assert_eq!(v.get("result").unwrap().as_str().unwrap(), "accepted");
    m.assert();

    // Submit signed tx to Anvil directly and ensure mining
    let pending = provider.send_raw_transaction(Bytes::from(raw)).await.expect("send raw");
    let receipt = pending.await.expect("mined");
    assert!(receipt.is_some());
}
