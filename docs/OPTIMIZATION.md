# Optimization & Latency Tuning Guide ⚡

## Benchmarks
- Use the provided Criterion benchmark (`benches/latency.rs`) to measure transaction build and signing latencies.
- Run: `cargo bench` (requires nightly or appropriate setup for async benches).

## Profiling
- Use `perf`, `pprof` or `Flamegraph` to profile hot paths in production-like environment.
- Annotate hot functions with short traces using `tracing` and export spans to get APM-level visibility.

## System tuning tips
- CPU: use dedicated cores for hot-path threads and pin critical tasks (e.g., signing, submission) with `taskset` / cgroups.
- IRQ / NIC: set IRQ affinity to isolate network interrupts and use `ethtool` to tune NIC offloads if necessary.
- TCP: tune `net.core.rmem_max`, `net.core.wmem_max`, `net.ipv4.tcp_rmem`, `net.ipv4.tcp_wmem` and set `tcp_congestion_control` appropriately.
- Use low-latency Linux kernels where possible and colocate near your RPC/validator endpoints.

## Code-level tips
- Pre-build and pre-serialize transactions off the hot path. Only attach nonce/gas at submission time.
- Use object pools and pre-allocated buffers to avoid repeated allocations in the signing loop.
- Keep WebSocket connections persistent and multiplex subscriptions.
- Measure P50/P95/P99 for the full pipeline: observe → decision → sign → submit.

## Monitoring
- Log P95/P99 latencies and set alerts for regression. Record end-to-end histograms for decision-to-submission latency.
