use criterion::{criterion_group, criterion_main, Criterion};
use mev_arbitrage_bot::tx::build_eip1559_tx;
use mev_arbitrage_bot::signer::BasicEnvSigner;
use ethers_core::types::{U256, Address, Bytes};

fn bench_tx_build(c: &mut Criterion) {
    c.bench_function("build_eip1559_tx", |b| {
        b.iter(|| {
            let _ = build_eip1559_tx(
                U256::from(0u64),
                Address::zero(),
                U256::from(0u64),
                Bytes::from(vec![]),
                U256::from(21000u64),
                U256::from(1_000_000_000u64),
                U256::from(100_000_000_000u64),
                1u64,
            );
        })
    });
}

fn bench_sign_tx(c: &mut Criterion) {
    // Uses BasicEnvSigner; set PRIVATE_KEY env var for benching
    if std::env::var("PRIVATE_KEY").is_err() {
        eprintln!("Skipping sign benchmark; set PRIVATE_KEY env var");
        return;
    }
    let secret = std::env::var("PRIVATE_KEY").unwrap();
    let signer = BasicEnvSigner::from_secret(secret);
    let tx = build_eip1559_tx(
        U256::from(0u64),
        Address::zero(),
        U256::from(0u64),
        Bytes::from(vec![]),
        U256::from(21000u64),
        U256::from(1_000_000_000u64),
        U256::from(100_000_000_000u64),
        1u64,
    );

    c.bench_function("sign_typed_tx", |b| {
        b.to_async(tokio::runtime::Runtime::new().unwrap()).iter(|| async {
            let _ = signer.sign_typed_transaction(&tx).await.unwrap();
        })
    });
}

criterion_group!(benches, bench_tx_build, bench_sign_tx);
criterion_main!(benches);
