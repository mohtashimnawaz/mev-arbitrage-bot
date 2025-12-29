# Testnet Canary Runbook

Purpose: Validate end-to-end system behavior with tiny capital before any mainnet deployment.

Prereqs:
- A funded testnet wallet with minimal ETH for gas (e.g., Goerli/Sepolia)
- `PRIVATE_KEY` set in the environment (dev-only)
- Optional: `FLASHBOTS_RELAY_URL` to a test relay
- `ANVIL_RPC_URL` if using a local fork

Steps:
1. Set env vars and start the bot in a controlled environment: `PRIVATE_KEY=... cargo run -- run`
2. Monitor logs and metrics; ensure no unexpected trades occur. Set an alert threshold for any trade above a tiny size.
3. If a trade is executed unexpectedly, hit the kill switch (stop process) and investigate logs.
4. Gradually increase exposure only after stable behavior for N blocks.

Safety checks:
- Daily and per-trade limits
- Max gas limits and fee caps
- Auto-kill on repeated simulation mismatches or high revert rate
- Audit logs retained for at least 30 days

Notes:
- Keep keys offline where possible and use a remote signer for production.
- Consult legal before moving to mainnet or handling third-party funds.
