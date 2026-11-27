# Token Manager

A read-only terminal dashboard for AI accounts: balances, spend and subscription
quotas. No inference routing, proxy, model execution, top-ups or billing changes.

This is a Linux CLI spike. Its provider adapters are TOML files, not branches in
the HTTP engine. Vercel is verified against a real account; OpenCode Go and Zen
are deliberately separate products and provider IDs.

## Try it

```sh
nix-shell
make check
make demo                     # Synthetic local HTTP data; no real accounts
make build
./target/release/token-manager providers
./target/release/token-manager status
```

Your existing Vercel account has been registered as `vercel`, referencing its
existing private credential file rather than duplicating its contents. Use
`status vercel --refresh` for live data. No account identifiers or secrets are
included in this repository.

For another account:

```sh
token-manager add vercel --name personal
# Shows provider-specific key guidance, prompts without echo, saves in OS keyring.
token-manager add opencode-go --name coding --env OPENCODE_GO_API_KEY
token-manager add opencode-zen --name wallet  # Bookmark + honest unsupported state

token-manager status
token-manager status personal --refresh
token-manager status --cached                # No account network requests
token-manager status --json
token-manager login personal                 # Reconnect/change credential
token-manager open personal billing
token-manager open personal keys
```

Use the built binary's path or install it yourself; setup does not change PATH.
`--open` on add/login opens the provider key page using `xdg-open`. There is no
browser OAuth implementation yet: `login` currently means connecting a key or
key reference, not signing into the provider's website automatically.

Credential options are mutually exclusive:

- Default: hidden interactive input saved through Linux Secret Service using
  `secret-tool`. Requires an unlocked desktop keyring/session bus; the Nix shell
  supplies the client but does not create or unlock a keyring daemon.
- `--env VARIABLE`: store only the environment variable's name.
- `--key-file /absolute/path`: reference an existing regular, non-symlink private
  file (0600 or stricter); the tool never modifies it.
- `--key-stdin`: read a key from stdin and save to the OS keyring. The CLI has no
  raw `--key VALUE` argument, keeping keys out of shell arguments/history.

No silent fallback stores keys as plaintext. Account configuration and cached
balances are private files; credentials remain separate. There is no automatic
import of OpenCode/Claux auth stores or borrowing a Go key for Zen.

## Model prices

```sh
token-manager prices claude                    # Compare a family, highest output rate first
token-manager prices gemini --sort input       # Highest input rate first
token-manager prices anthropic/claude-sonnet-4  # Exact catalog model ID
token-manager prices gpt --json
```

Fetches the public OpenRouter text-model catalog without an account or credentials.
Input and output prices are shown separately in USD per million tokens. These are
OpenRouter catalog rates, not a promise of direct-provider pricing or your final
bill; caching, context tiers, routing, and non-token charges may differ. The command
fetches fresh data on each run and does not write account configuration or caches.

Matching ignores case, spaces, and punctuation. Exact IDs, short model slugs, and
display names take precedence; otherwise the query matches part of an ID or name.
Unknown, malformed, negative/dynamic, and missing rates stay unavailable and sort
last; an explicit zero remains free. No matches or fetch failures return an error.
JSON includes the source, observation time, currency, sort field, and both rates.
`--provider` selects a provider with a public `[pricing]` mapping (currently only
OpenRouter). Adding another source requires a TOML mapping with USD-per-token rates.

## Providers

| ID | Account data | Status |
|---|---|---|
| `vercel` | AI Gateway remaining credits + total used | Official endpoint; live verified |
| `opencode-go` | Rolling, weekly, monthly usage percentages/reset times | Experimental endpoint; live quota check verified |
| `opencode-zen` | Separate prepaid credit wallet | No verified public balance API; billing link only |
| `openrouter` | Purchased minus used credits | Requires a management key; fixtures verified |
| `deepseek` | Per-currency balance | Official mapping; fixtures verified |
| `moonshot` | International Kimi API credits in USD | Official mapping; fixtures verified |
| `moonshot-cn` | China Kimi API credits in CNY | Separate account/endpoint; fixtures verified |
| `featherless` | Spendable Developer credits after reservations | Admin billing permission required; fixtures verified |
| `synthetic` | Absolute subscription request usage/limit + percentage | Experimental API; fixtures verified |
| `nanogpt` | USD wallet balance | Read-only POST; official schema; fixtures verified |
| `nanogpt-subscription` | Daily/weekly input-token and daily image allowances | Separate subscription counters; fixtures verified |
| `chutes` | Account credit balance | Upstream account-read mapping; fixtures verified |

Go windows do not describe the Zen wallet. Missing data says “not available”;
unsupported or failed checks never become a fabricated zero balance.
[Provider definitions and sources](docs/providers.md) explain these boundaries.

## Behavior and architecture

`CLI → account references + provider definitions → bounded account request → normalization → table/JSON`

Built-in adapters are embedded automatically from the sorted `providers/*.toml`
directory during compilation. Adding a service requires a definition and fixtures,
not editing a Rust registry or adding provider branches.

The engine supports four concurrent account checks, eight-second HTTP timeouts,
a 1 MiB response limit, exact decimal money/quota arithmetic and same-origin HTTPS
requests without redirects. Configurable auth headers/prefixes cover bearer and
API-key header conventions; provider files cannot contain executable scripts.

Successful data is cached for 60 seconds. Fresh cache returns immediately;
`--refresh` forces a check. On failure the previous successful data remains visible
with an explicit stale label and observation age. Error bodies are not printed.
JSON includes state, timestamps, cached/stale flags and normalized data.

Custom definitions live under `~/.config/token-manager/providers/*.toml`.
Account config is `accounts.json` and successful snapshots are under `cache/`.
`TOKEN_MANAGER_CONFIG_DIR` or `--config-dir` selects an isolated configuration;
`XDG_CONFIG_HOME` is supported. Definitions are trusted endpoint configuration.
A changed definition requires explicit reconnect before credentials are sent.
Concurrent account edits use a lock; a crash can leave `accounts.lock` for manual
inspection after confirming no edit is still running. Status reads are unlocked.

Exit codes: **0** successful command/all account checks ready or cached; **1**
invalid command/configuration or setup failure; **2** one or more account checks
need attention or are unsupported. Partial status still prints all rows/JSON.

## Development and verification

`Cargo.toml` owns version 0.1.0; `Cargo.lock` locks dependencies. `shell.nix` pins
nixpkgs and declares Rust/Cargo, format/lint tools, Python, Make and libsecret.
`make check` runs formatting, Clippy with warnings denied and behavioral tests.
Tests use disposable state, synthetic credentials and local HTTP servers; they
never contact real providers. The keyring fixture substitutes a local Secret
Service client; it does not prove this machine's desktop keyring is unlocked.

The demo creates its own provider overrides and accounts, validates normalized
results and cleans up its own directory/server. It does not seed real accounts.
This headless CLI uses repeatable text output rather than a screenshot workflow.
Docker, a daemon, a database and an inference SDK are unnecessary for this spike.

CI calls the same Make targets through the pinned shell. Local validation is
recorded in [the implementation report](docs/implementation.md); remote CI has
not run. [Release instructions](docs/releases.md) distinguish safe dry-run validation,
local archive preparation and explicit crates.io publication with `make release`.

## Remaining scope

No OAuth/device login or refresh, TUI/watch mode, account removal, credit-expiry
lots, automatic purchase, provider scraping or quota forecasting. Custom mappings
cover one GET or empty read-only account POST, JSON pointers, scalar response
checks, percent/used-limit quotas, explicit epoch reset formats and degraded-counter
guards; pagination, signing, multi-step requests and
nonstandard authentication require a later explicit capability. Built-in endpoint
support is not a guarantee every key/plan has the required permission.

See [the openai account guide](docs/openai.md) for the new API and subscription adapters.
