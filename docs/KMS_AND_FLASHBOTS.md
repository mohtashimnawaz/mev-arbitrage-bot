KMS & Flashbots Verification

KMS & HSM

- `src/kms.rs` exposes the `KmsClient` trait and a `MockKms` implementation for development and tests.
- `src/kms/aws.rs` and `src/kms/yubihsm.rs` contain provider skeletons. These are intentionally placeholders — implementors should add concrete integrations (AWS SDK, YubiHSM SDK) and feature-gate them in `Cargo.toml` for production use.
- **Security note:** Do not store raw private keys in code or environments for production; use proper HSM/KMS and secrets management.

Live Flashbots simulate verification

- A test `tests/live_flashbots_verify.rs` performs a live comparison between a configured relay's `eth_simulateBundle` response and a local Anvil simulation.
- To run the test, set the following env vars and run the ignored test:
  - `RUN_FLASHBOTS_VERIFY=1`
  - `FLASHBOTS_RELAY_URL`
  - `ANVIL_RPC_URL`
  - `PRIVATE_KEY`
- The test is intentionally ignored by default and gated by `RUN_FLASHBOTS_VERIFY=1` to prevent accidental network calls. It reports discrepancies between relay-side and local simulation results for further investigation.

Autosubmit monitoring

- `src/autosubmit.rs` contains a simple autosubmitter that attempts relay submission (if configured), falls back to direct provider submission, and polls for transaction inclusion. It supports basic resubmission attempts and configurable timeouts.
- Future improvements: re-broadcast with gas bumping strategies, integration with relay-specific APIs, and stronger safety checks (e.g., cost-based kill switch and operator alerts).
