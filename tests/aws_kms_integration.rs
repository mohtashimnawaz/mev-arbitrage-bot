#![cfg(feature = "aws-kms")]

use mev_arbitrage_bot::kms::aws::real::AwsKmsClient;

#[tokio::test]
#[ignore]
async fn aws_kms_get_public_key_integration() {
    if std::env::var("RUN_AWS_KMS_INTEGRATION").unwrap_or_default() != "1" {
        eprintln!("Skipping AWS KMS integration test: set RUN_AWS_KMS_INTEGRATION=1 and AWS_KMS_KEY_ID and AWS_REGION");
        return;
    }
    let key_id = match std::env::var("AWS_KMS_KEY_ID") {
        Ok(v) => v,
        Err(_) => { eprintln!("Skipping: AWS_KMS_KEY_ID not set"); return; }
    };

    let client = AwsKmsClient::from_env(key_id).await.expect("failed to construct KMS client");
    let pk = client.get_public_key().await.expect("failed to get public key");
    assert!(pk.len() > 0);
}
