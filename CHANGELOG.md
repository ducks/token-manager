# Changelog

## Unreleased

- Add the `tm` executable and `--vendor` pricing browser, with direct vendor offers
  first and third-party prices grouped under canonical model IDs.
- Compare Models.dev catalog prices across providers, preserving context tiers,
  unavailable rates and distinct preview/model identities. Retain the live
  OpenRouter-only catalog through `--source openrouter`.
- Include both executable names in local release archives.

## 20261127.0.0

- Add `prices <model-or-family>` using the public OpenRouter catalog, separate
  input/output USD-per-million rates, price sorting, exact-model matching and JSON.
- Add OpenAI and Anthropic organization cost reports with bounded pagination,
  exact decimal conversion and explicit 30-day UTC reporting windows.
- Add experimental ChatGPT/Codex and Claude subscription quota adapters using
  explicitly supplied OAuth access tokens, separate from API billing credentials.
- Add separate Gemini API and Google AI subscription dashboard bookmarks, with
  explicit unsupported live-data states rather than guessed balances or quotas.
- Preserve existing account bindings when adding public pricing metadata and
  document the new adapters, credential requirements and reporting limitations.

## 20261122.0.0

- Add date-versioned releases with verified crates.io packaging, release commits,
  annotated tags and atomic Git pushes. Keep local packaging as `release-archive`.

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
