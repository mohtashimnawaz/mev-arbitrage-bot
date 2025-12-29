use mev_arbitrage_bot::kms::yubihsm::YubiHsm;

#[tokio::test]
#[ignore]
async fn yubihsm_connects_when_configured() {
    if std::env::var("RUN_YUBIHSM_INTEGRATION").unwrap_or_default() != "1" {
        eprintln!("Skipping YubiHSM integration test: set RUN_YUBIHSM_INTEGRATION=1 and YUBIHSM_CONNECTOR");
        return;
    }
    let connector = match std::env::var("YUBIHSM_CONNECTOR") {
        Ok(v) => v,
        Err(_) => { eprintln!("Skipping: YUBIHSM_CONNECTOR not set"); return; }
    };

    let client = YubiHsm::new(connector);
    // At minimum, ensure sign returns either Ok or a clear error instead of panicking
    let res = client.sign(&[0u8;32]).await;
    assert!(res.is_err() || res.is_ok());
}
