# Logfire Token Tracking

## Summary

Review Logfire's built-in features for tracking LLM token usage and costs. The Rust SDK
may support structured token/cost reporting that we could use instead of manually
logging `input_tokens` / `output_tokens` fields.

## Motivation

- Centralized cost tracking across providers and models
- Logfire dashboard queries for token usage trends
- Potential per-session or per-job cost attribution

## Investigation Items

- Check if `logfire-rust` has dedicated LLM instrumentation (similar to the Python SDK's
  `logfire.instrument_openai()`)
- Evaluate whether Logfire's metrics support (`MetricsOptions`) can aggregate token
  counts as OpenTelemetry metrics
- Consider adding cache read/creation token tracking (already in `Usage` type but not
  logged)
