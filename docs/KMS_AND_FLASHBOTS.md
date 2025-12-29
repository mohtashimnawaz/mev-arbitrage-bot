KMS & Flashbots Verification

KMS & HSM

- `src/kms.rs` exposes the `KmsClient` trait and a `MockKms` implementation for development and tests.
- `src/kms/aws.rs` and `src/kms/yubihsm.rs` contain provider skeletons. These are intentionally placeholders — implementors should add concrete integrations (AWS SDK, YubiHSM SDK) and feature-gate them in `Cargo.toml` for production use.
- **Security note:** Do not store raw private keys in code or environments for production; use proper HSM/KMS and secrets management.

AWS KMS integration (feature: `aws-kms`)

- To enable AWS KMS support, enable the `aws-kms` Cargo feature. This pulls in `aws-config` and `aws-sdk-kms`.
- Requirements for the integration test:
  - Set `RUN_AWS_KMS_INTEGRATION=1` to run the ignored integration test.
  - Set `AWS_KMS_KEY_ID` to your KMS key id (the key must support ECDSA/secp256k1 for Ethereum signing).
  - Configure AWS credentials via the standard environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`) or via the AWS SDK's default credential chain.
- The integration test `tests/aws_kms_integration.rs` calls `GetPublicKey` and validates that a public key is returned. Signing still requires ECDSA support and the calling code must adapt DER signatures to Ethereum's (r, s, v) format for tx signing.

YubiHSM integration (feature: `yubihsm`)

- The `yubihsm` feature flag can be enabled when you have a YubiHSM available and the `yubihsm` Rust crate provides a compatible client. The repository contains a skeleton implementation in `src/kms/yubihsm.rs` and an ignored test scaffold.
- Consult vendor documentation for test harness and connector setup.

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
