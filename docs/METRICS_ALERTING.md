Prometheus alerting rules and Grafana dashboard suggestions

Alerts (Prometheus rule file snippet):

groups:
  - name: mev-arb-alerts
    rules:
      - alert: KmsSignFailures
        expr: sum(rate(kms_sign_failure_total[5m])) by (key_id) > 0
        for: 1m
        labels:
          severity: critical
        annotations:
          summary: "KMS signing failures detected for key {{ $labels.key_id }}"
          description: "High rate of KMS signing failures. Investigate KMS availability and permissions."

      - alert: AutosubmitHighResubmissions
        expr: rate(autosubmit_resubmissions_total[5m]) > 1
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "High autosubmitter resubmission rate"
          description: "Autosubmitter is retrying bundles frequently. Check network/relay issues and recent inclusion failures."

      - alert: KmsSignLatencyHigh
        expr: histogram_quantile(0.95, rate(kms_sign_duration_seconds_bucket[5m])) > 1.0
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High KMS sign latency (95th percentile)"

Grafana dashboard suggestions

- Panel: KMS Sign Success/Failure
  - Graph counters: `kms_sign_attempts_total`, `kms_sign_success_total`, `kms_sign_failure_total`
  - Alert: KmsSignFailures rule

- Panel: Autosubmitter activity
  - Graph `autosubmit_submissions_relay_total`, `autosubmit_resubmissions_total`, `autosubmit_inclusions_total`
  - Table: recent resubmission events (if event logging available)

- Panel: KMS sign latency
  - Histogram/summary view using `kms_sign_duration_seconds` buckets

Instrumentation notes

- Current code uses `metrics` counters under the `with-metrics` feature. To enable Prometheus scraping:
  - Enable feature `with-metrics` and run a `metrics-exporter-prometheus` HTTP server to expose `/metrics`.
  - Configure Prometheus to scrape that endpoint and add the alerting rules above to your Prometheus server.
