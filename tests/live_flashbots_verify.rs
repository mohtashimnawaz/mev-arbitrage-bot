use mev_arbitrage_bot::executor::RelayClient;
use mev_arbitrage_bot::sim::Simulator;
use mev_arbitrage_bot::tx::build_eip1559_tx;
use mev_arbitrage_bot::signer::BasicEnvSigner;
use ethers_core::types::{U256, Address, Bytes, transaction::eip2718::TypedTransaction};

#[tokio::test]
#[ignore]
async fn live_flashbots_simulation_matches_local_anvil() {
    // This test only runs when RUN_FLASHBOTS_VERIFY=1 and required env vars are present
    if std::env::var("RUN_FLASHBOTS_VERIFY").unwrap_or_default() != "1" {
        eprintln!("Skipping live Flashbots verify test: set RUN_FLASHBOTS_VERIFY=1");
        return;
    }

    let flash_url = match std::env::var("FLASHBOTS_RELAY_URL") {
        Ok(v) => v,
        Err(_) => { eprintln!("Skipping: FLASHBOTS_RELAY_URL not set"); return; }
    };
    let anvil = match std::env::var("ANVIL_RPC_URL") {
        Ok(v) => v,
        Err(_) => { eprintln!("Skipping: ANVIL_RPC_URL not set"); return; }
    };

    let private = match std::env::var("PRIVATE_KEY") {
        Ok(v) => v,
        Err(_) => { eprintln!("Skipping: PRIVATE_KEY not set"); return; }
    };

    // Build a trivial tx and sign it
    let provider = ethers_providers::Provider::<ethers_providers::Http>::try_from(anvil.as_str()).unwrap();
    let chain_id = provider.get_chainid().await.unwrap().as_u64();
    let tx = build_eip1559_tx(
        U256::from(0u64),
        Address::zero(),
        U256::from(0u64),
        Bytes::from(vec![]),
        U256::from(21000u64),
        U256::from(1_000_000_000u64),
        U256::from(100_000_000_000u64),
        chain_id,
    );

    let signer = BasicEnvSigner::from_secret(private);
    let signed = signer.sign_typed_transaction(&tx).await.unwrap();
    let signed_blob = vec![signed.clone()];

    // 1) Simulate locally on Anvil
    let sim = Simulator::new();
    let local_receipts = sim.simulate_signed_bundle(&signed_blob, None).await.unwrap();

    // 2) Simulate via relay
    let rc = RelayClient::with_url(flash_url).unwrap();
    let relay_res = rc.simulate_flashbots_bundle(&signed_blob, None).await.unwrap();

    // Relay result shape is relay-dependent; try to extract status per tx if present
    // If the relay returns 'result' array with per-tx states, compare statuses conservatively.
    let mut relay_statuses: Vec<u64> = Vec::new();
    if let Some(arr) = relay_res.get("result") {
        if let Some(a) = arr.as_array() {
            for item in a.iter() {
                if let Some(obj) = item.as_object() {
                    if let Some(status) = obj.get("status").and_then(|v| v.as_u64()) {
                        relay_statuses.push(status);
                    }
                }
            }
        }
    }

    // Compare at least that local receipts exist and relay returned something
    assert!(local_receipts.len() == 1);
    assert!(relay_res.is_object());

    // If relay provided statuses, they should match local receipts' status where available
    if !relay_statuses.is_empty() {
        let local_status = local_receipts[0].status.unwrap_or_default().as_u64();
        assert_eq!(relay_statuses[0], local_status);
    }
}
