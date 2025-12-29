# Security & Key Management

- **Never store raw private keys on disk in production.** Use HSMs, cloud KMS or a remote signer.
- Use hardware signer stubs only for development and testing.
- Enforce least-privilege for any deployed keys and rotate regularly.
- Implement rate-limits and withdrawal caps if managing user funds.
- Keep full audit logs for transaction decisions (ensure PII/key material is never logged).
- Add automated tests for replay protection and nonce handling to prevent accidental double-spends.
