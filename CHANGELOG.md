# Changelog

## Unreleased

- Add separate NanoGPT wallet/subscription and Chutes account-balance adapters.
- Support empty read-only account POST requests, explicit epoch reset formats
  and degraded usage guards without changing existing default adapter bindings.

- Add Kimi/Moonshot international and China balances, Featherless spendable
  credits and experimental Synthetic subscription limits.
- Embed built-in provider definitions automatically without a Rust registry edit.
- Add absolute quota counts, derived percentages and scalar response guards,
  preserving missing, zero and over-limit states explicitly.

- Add read-only Rust account CLI with provider TOML definitions and exact balances.
- Add Vercel credits and distinct experimental OpenCode Go/unsupported Zen entries.
- Add OpenRouter management-key and DeepSeek per-currency mappings.
- Add keyring, environment and private-file credential references; cached and JSON status.
- Add local HTTP/keyring/CLI tests, synthetic demo and local release preparation.
