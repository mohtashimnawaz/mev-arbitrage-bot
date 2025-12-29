# mev-arbitrage-bot

A low-latency, ethical MEV/arbitrage bot MVP in Rust.

Goals:
- Ethical arbitrage & liquidation capture
- Low-latency observation → decision → private bundle submission
- Safety-first: re-simulation, limits, and HSM-backed signing

Quick start:

1. Install Rust stable toolchain.
2. Build: `cargo build --release`
3. Run (stub): `cargo run -- run` (starts background feed + scanner)

This repo contains early scaffolding and module stubs for the MVP.

Development:

- Run unit tests: `cargo test`
- Run the simulator (stub): `cargo run -- simulate`

See `src/` for modules: `config`, `data`, `scanner`, `signer`, `executor`, and `sim`.
