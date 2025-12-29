# Instrumentation & Metrics

## Key metrics
- Observation → Decision → Submission latency (P50/P95/P99)
- Strategy counts: opportunities detected vs executed
- Execution success rate (no revert) and gas/relay cost per trade
- P&L per strategy, per time window
- Node/relay latency and error rates

## Tracing points
- Market data receive timestamp
- Strategy decision timestamp and inputs
- Simulation start/end and outcome
- Transaction build time and signing time
- Bundle submission time and relay response

## Monitoring
- Use Prometheus + Grafana to collect metrics
- Set alerts for high revert rates, latency spikes, or P&L drawdowns
- Integrate Sentry for error reporting and structured logs

## Logging
- Structured logs with tracing; include trace IDs for cross-service correlation
- Log all decisions and inputs for compliance and forensics (redact keys)

## Benchmarks
- Continuously benchmark end-to-end and per-stage latencies; keep historical baselines
- Run synthetic tests with injected delays to validate resilience
