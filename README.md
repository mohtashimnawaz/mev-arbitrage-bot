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
- Run the bot (stub): `cargo run -- run` (starts background feed + scanner)
- Run the simulator (stub): `cargo run -- simulate`

Environment vars (dev only):
- `PRIVATE_KEY` — a local private key for testing the `BasicEnvSigner` (DO NOT store keys in repo)
- `FLASHBOTS_RELAY_URL` — optional relay endpoint for private bundle submission
- `ANVIL_RPC_URL` — forked node RPC URL for the simulator (default: `http://127.0.0.1:8545`)

See `src/` for modules: `config`, `data`, `scanner`, `signer`, `executor`, and `sim`.
