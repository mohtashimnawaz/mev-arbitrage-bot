# Architecture Overview

## Components

- Market Data Layer
  - WebSocket and RPC subscribers to multiple providers
  - Quote normalization and caching
  - Provider failover

- Mempool Observer (optional)
  - Pending transaction stream
  - Gas market and auction monitoring

- Scanner / Strategy Engine
  - Cross-DEX arb, triangular arb, liquidation detection
  - Profitability calc, fee/slippage modeling, safety thresholds

- Simulator & Safety Checks
  - Forked mainnet simulation (Anvil/Hardhat)
  - Re-simulation for every candidate trade

- Tx Builder & Signer
  - Pre-serialize transactions and bundles
  - Secure signing abstraction (HSM/KMS-backed in production)

- Executor / Relay Client
  - Flashbots-style private bundle submission
  - Retry and bundle monitoring

- Telemetry & Ops
  - Latency tracing, P95/P99 metrics, P&L dashboard
  - Kill-switch, capital limits, audit logs

## Data flow
1. Market data ingestion (WS/RPC) → normalized events
2. Strategy scanner triggers candidate trade
3. Re-simulate trade on forked node and validate safety checks
4. Build and sign bundle/tx
5. Submit to relay or public mempool
6. Monitor inclusion and handle failures

## Notes
- Keep critical path minimal and async. Rust + tokio chosen for lower GC overhead and determinism.
- Use private bundle submission to avoid public front-running where appropriate.
- Add thorough logging to all decisions for auditing and compliance.
