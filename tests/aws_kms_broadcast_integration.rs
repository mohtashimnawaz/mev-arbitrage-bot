#![cfg(feature = "aws-kms")]

use mev_arbitrage_bot::kms::aws::real::AwsKmsClient;
use mev_arbitrage_bot::crypto::der::der_to_ethers_signature;
use mev_arbitrage_bot::tx::build_eip1559_tx;
use ethers_providers::{Provider, Http};
use ethers_core::types::{U256, Address, Bytes, transaction::eip2718::TypedTransaction};

#[tokio::test]
#[ignore]
async fn aws_kms_sign_and_broadcast_to_anvil() {
    if std::env::var("RUN_AWS_KMS_SIGN_BROADCAST").unwrap_or_default() != "1" {
        eprintln!("Skipping AWS KMS sign+broadcast integration test: set RUN_AWS_KMS_SIGN_BROADCAST=1 and AWS_KMS_KEY_ID and ANVIL_RPC_URL and AWS creds");
        return;
    }
    let key_id = match std::env::var("AWS_KMS_KEY_ID") {
        Ok(v) => v,
        Err(_) => { eprintln!("Skipping: AWS_KMS_KEY_ID not set"); return; }
    };

    let anvil = match std::env::var("ANVIL_RPC_URL") {
        Ok(v) => v,
        Err(_) => { eprintln!("Skipping: ANVIL_RPC_URL not set"); return; }
    };

    let client = AwsKmsClient::from_env(key_id).await.expect("failed to construct KMS client");
    let expected_addr = client.get_address().await.expect("get_address call failed").expect("public key did not yield address");

    // Build a simple tx and compute sighash
    let tx = build_eip1559_tx(
        U256::from(0u64),
        expected_addr,
        U256::from(0u64),
        Bytes::from(vec![]),
        U256::from(21000u64),
        U256::from(1_000_000_000u64),
        U256::from(100_000_000_000u64),
        1u64,
    );

    let sigh = tx.sighash();
    let der = client.sign_digest(sigh.as_bytes()).await.expect("sign failed");
    let sig = der_to_ethers_signature(&der, sigh.as_bytes(), Some(expected_addr)).expect("DER->ethers signature failed");

    // RLP sign and broadcast
    let raw = tx.rlp_signed(&sig);
    let provider = Provider::<Http>::try_from(anvil.as_str()).expect("invalid anvil url");
    let pending = provider.send_raw_transaction(Bytes::from(raw)).await.expect("send failed");
    let receipt = pending.await.expect("reception failed");
    assert!(receipt.is_some());
}
