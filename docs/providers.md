# Declarative providers

Create a TOML file under the configuration's `providers/` directory. Definitions
are full replacements, not partial merges. Duplicate custom IDs fail. A custom
ID may override a built-in; reconnect is required when its fingerprint changes.
Account credential bindings and cache fingerprints include the account definition,
excluding public pricing metadata.
Built-ins are automatically embedded from the sorted repository `providers/*.toml`
directory by `build.rs`; new services need no Rust registry edit. Custom user
configuration files remain dynamically loaded.
Do not install unreviewed provider files or put actual credentials in them.

```toml
id = "example"
name = "Example credits"
base_url = "https://api.example.com"
path = "/account/credits"
key_url = "https://example.com/settings/keys"
billing_url = "https://example.com/billing"
credential_help = "Use an account-read key."
source = "https://example.com/api/docs"

[auth]
header = "Authorization"  # default
prefix = "Bearer "        # default; use prefix = "" for X-API-Key

[balance]
rows = "/data"
currency = "USD"
remaining = "/balance"
used = "/spent"           # optional
```

JSON paths are RFC 6901 JSON pointers, not JSONPath or executable expressions.
`rows` resolves to an object or array. Use `currency_path` instead of `currency`
for multiple currencies. Use `total` and `used` instead of `remaining` to compute
exact `total - used`. Required fields that are missing or malformed fail; they
are not coerced to zero. Negative remaining credit is preserved as an overdraft.
Never sum different currencies into one total.

Subscription windows can be mapped independently:

```toml
[[quotas]]
label = "Weekly"
percent = "/usage/weekly/percent" # percent USED, not fraction or remaining
reset_at = "/usage/weekly/resetsAt"
```

For absolute quota counters, replace `percent` with `used`, `limit` and `unit`:

```toml
[[quotas]]
label = "Subscription"
used = "/subscription/requests"
limit = "/subscription/limit"
unit = "requests"
reset_at = "/subscription/renewsAt"
```

Used/limit fields must be paired and have an explicit unit. Checked decimal
arithmetic derives `used / limit * 100`; zero limits leave percent unknown rather
than implying unlimited allowance. Missing counters stay unknown, and over-limit
percentages are not clamped. An API can expose direct percent and absolute counts
together. This does not invent absolute allowances for APIs returning only percent.

Optional fixed response guards reject in-band errors despite HTTP 200:

```toml
[[response_checks]]
path = "/code"
equals = 0
[[response_checks]]
path = "/status"
equals = true
```

Every declared scalar equality must match. Missing/mismatched guards fail mapping;
these checks cannot run scripts or arbitrary expressions.

Missing windows show unknown. At least one mapped balance or quota must exist
in a successful response. Reset timestamps default to text displayed as returned. Set `reset_format` to
`unix_seconds` or `unix_milliseconds` to normalize integer epoch values into UTC
RFC 3339 timestamps. Countdown rendering is not implemented. Set
`unavailable_when` to a boolean JSON pointer to suppress a degraded window’s
used count, percentage and reset while retaining its declared limit. Unit scaling and arbitrary arithmetic are not supported beyond the declared
balance subtraction and used/limit percentage calculation. Decimal strings and JSON numbers are accepted for amounts/percentages.

Requests default to GET. `method = "POST"` supports an empty request to a
verified read-only account endpoint, such as NanoGPT wallet lookup. Definitions
cannot send arbitrary request bodies; do not configure mutation or inference
routes. Other methods are rejected. HTTPS is required; redirects are refused.
`--allow-local-http` permits only literal 127.0.0.1 or ::1 fixture addresses,
not arbitrary HTTP hosts. Request paths cannot switch the configured origin.
Errors expose bounded categories, never raw provider response bodies.

A provider without a verified account endpoint uses `unsupported_reason` instead
of path/data mappings. It can be added without credentials as an account bookmark.
There is no fallback that substitutes another product's quota or guessed endpoint.

## Verified research / boundaries

- Vercel's maintained [spend reference](https://github.com/vercel/vercel-plugin/blob/main/skills/ai-gateway/references/spend-observability.md)
  documents `GET https://ai-gateway.vercel.sh/v1/credits`: `balance`, `total_used`.
  Authentication uses the team's [AI Gateway key](https://vercel.com/docs/ai-gateway/authentication-and-byok/api-keys).
  This spike successfully called that endpoint with the user's existing key file.
- [OpenCode Go](https://opencode.ai/docs/console/go/) is a subscription, separate
  from prepaid Zen credits. The proposed window mapping for `/zen/go/v1/usage`
  is evidenced by [the upstream balance API issue](https://github.com/anomalyco/opencode/issues/44189).
  It is not a stable public billing contract. A live check with the saved Go API
  key succeeded and returned rolling, weekly and monthly percentages/reset times.
  Its sample `balance` object is a feature request, NOT an implemented endpoint;
  we deliberately do not map it or invent a `/zen/v1/balance` request.
- [OpenRouter credits](https://openrouter.ai/docs/api/api-reference/credits/get-remaining-credits)
  requires a management key. Browser-created inference keys do not automatically
  grant account-wide billing access.
- [DeepSeek balance](https://api-docs.deepseek.com/api/get-user-balance/)
  contains `balance_infos` with currency-specific `total_balance` values.

OAuth PKCE/device login should eventually be reusable auth engines configured by
provider definitions. No browser-session scraping or generic arbitrary-code
plugin loader is included in this prototype.

## Services added on feature/more-providers

- [Kimi International](https://platform.kimi.ai/docs/api/balance) and
  [Kimi China](https://platform.kimi.com/docs/api/balance) have separate API
  origins, accounts and currencies (USD/CNY). Map authoritative `available_balance`
  directly, not cash plus vouchers: negative cash has special semantics. These
  are API credits, not Kimi Code subscriptions. Both check the success envelope.
- [Featherless Developer credits](https://featherless.ai/docs/api-reference-credits)
  maps `available_usd` after in-flight reservations, not subtotal `balance_usd`.
  Organization Admin billing permission is required; 403 is not zero credit.
  This is not a Featherless Chat plan allowance and lifetime spend is not invented.
- [Synthetic /v2/quotas](https://dev.synthetic.new/docs/synthetic/quotas) maps the
  documented request usage, limit and renewal time. It remains experimental
  because the API is under development. Requests are not re-labeled as dollars
  or wallet credit, and the adapter does not assume a fixed allowance for all plans.

All four new definitions have synthetic schema-fixture coverage; no credentials
for these new services were used and no live success is claimed. Existing live
verification for Vercel/Go remains distinct from fixture coverage.

SiliconFlow's balance routes have region/version changes and need separate
validation rather than guessed mappings; they are not included in this catalog.

## NanoGPT and Chutes

- [NanoGPT OpenAPI](https://docs.nano-gpt.com/api-reference/openapi.json)
  specifies an API base of `https://api.nano-gpt.com/api`. The wallet uses an
  empty POST to `/check-balance`, an `x-api-key` header, and the authoritative
  `usd_balance` decimal string. Nano-denominated holdings are not converted
  into guessed USD credit.
- NanoGPT subscription usage is a separate GET to `/subscription/v1/usage`
  using an inference API key. Input-token and image counters retain their own
  units. Percentages are derived from used/limit counters, not interpreted
  directly from the API's fractional `percentUsed`. Reset times are epoch
  milliseconds; degraded counters remain unavailable. The management-token
  usage route is different and is not substituted automatically.
- [Chutes user router](https://github.com/chutesai/chutes-api/blob/main/api/user/router.py)
  supports GET `/users/me` using a credential with account-read access. Only
  `balance` is normalized; profile fields and payment addresses are excluded.
  Null/redacted balance is unavailable. These credits are not subscription
  request quotas, and invocation-only keys may lack account-read permission.

These three adapters have isolated schema and HTTP fixtures. Public documentation
was checked on November 22, 2026; no real NanoGPT or Chutes account was queried.

## Public pricing catalogs

An optional `[pricing]` table maps a public GET catalog independently of account
credentials: `path`, documentation `source`, and JSON pointers `rows`, `id`, `name`,
`input`, and `output`. Input/output values must be USD per token. The engine applies
the per-million conversion with decimal arithmetic. Catalog requests never send
account authentication and refuse redirects. Pricing metadata is excluded from
account fingerprints so a catalog update does not require reconnecting accounts.

OpenRouter mapping reference: https://openrouter.ai/docs/guides/overview/models

Organization spend adapters use the separate [cost-report mapping](cost-adapters.md).
See the [OpenAI](openai.md), [Anthropic](anthropic.md), and [Google](google.md)
guides for API/subscription credential boundaries and unsupported meters.

## Cross-provider model pricing

`providers/catalogs/models-dev.toml` configures the public comparison catalog,
separately from account adapters. The same relative path under the user config
directory can override it for local fixtures or a compatible catalog. It declares
the HTTPS endpoint, attribution URL, JSON pointers for provider/model dictionaries,
canonical model IDs and per-million USD costs. `direct_providers` maps additional
first-party platforms to their model vendors; `vendor_aliases` controls CLI aliases.

The reader preserves context tiers, provider IDs, status and unknown rates, and
uses canonical IDs rather than fuzzy matching to join offers. Identity-less offers
on multi-vendor hosts are not attributed to that host's owner. This configuration
is independent of account fingerprints and never supplies authentication.
Requests refuse redirects and are capped at 32 MiB with a 30-second timeout.

Sources/schema: https://github.com/anomalyco/models.dev#api and
https://github.com/anomalyco/models.dev/blob/dev/packages/core/src/schema.ts
