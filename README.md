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

Both `tm` and `token-manager` run the same CLI:

```sh
tm --vendor google                            # All listed Google models and their offers
tm prices gemini                              # Search a model family across providers
tm prices google/gemini-2.5-pro                # Exact model, all linked provider offers
tm --vendor google --provider openrouter      # Only OpenRouter offers for Google models
tm prices claude --sort input --json
tm prices gemini --source openrouter           # Previous live OpenRouter-only catalog
```

**Vendor** means the company behind the model; **provider** means the service
selling access to it. Results group offers under each model: the vendor's primary
API comes first, then other first-party platforms (such as Google Vertex), then
third parties. Each section sorts by highest input or output price; model groups
sort by their first available offer's rate. Input/output and cache-read rates are
shown separately in USD per million text tokens, with context pricing tiers on
separate rows. JSON also includes cache-write rates, provider model IDs and links.

The default source is the public [Models.dev catalog](https://models.dev), which
contains direct API prices and third-party offers. These are community-maintained
catalog rates, **not live vendor quotes**. They may lag vendor changes; the fetch
time is not a price-verification date. The command shows all catalog-listed offers
it can link, not every possible reseller. Provider subscriptions, batch discounts,
mode-specific pricing and non-token charges can change the actual bill. Non-text
or unspecified-output models remain listed, but their token rates are unavailable
rather than being assigned a potentially incorrect unit. Deprecated offers are
marked. The catalog can also lack a direct-vendor offer; that is shown explicitly.

Canonical catalog model IDs join offers. Preview, dated and other distinct IDs
stay separate; unlinked offers are never guessed from similar names. Unlinked
first-party models retain their vendor; other unlinked offers are available with
`--provider` but cannot match `--vendor`. `gemini`, `claude` and `gpt` are accepted
vendor aliases for `google`, `anthropic` and `openai`. Search ignores case, spaces
and punctuation; exact IDs or names take precedence over family/substring matching.

No account or credentials are needed. Each invocation fetches fresh catalog data
without writing account state or caches. Missing, malformed and negative rates
remain unavailable; an explicit zero is retained, not a guarantee of free access.
No matches or fetch failures return an error. The JSON schema for the default
source groups `offers` within `models`; `--source openrouter` retains the old flat
response and requires a model/family query. `--provider` now filters serving
providers; select the live OpenRouter catalog with `--source openrouter` instead.

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
| `openai` | Organization spend over 30 completed UTC days | Admin key; fixtures verified |
| `chatgpt` | Primary/secondary subscription quota windows | Explicit OAuth token; experimental; fixtures verified |
| `anthropic` | Organization spend over 30 completed UTC days | Admin key; beta analytics API; fixtures verified |
| `claude` | Session, weekly, and weekly Sonnet quotas | Explicit OAuth token; experimental; fixtures verified |
| `gemini` | Gemini API billing bookmark | Live balance/spend unsupported |
| `gemini-subscription` | Google AI / Gemini subscription bookmark | Live quota unsupported |

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
A changed account definition requires explicit reconnect before credentials are sent;
public pricing metadata is excluded from that binding.
Concurrent account edits use a lock; a crash can leave `accounts.lock` for manual
inspection after confirming no edit is still running. Status reads are unlocked.

Exit codes: **0** successful command/all account checks ready or cached; **1**
invalid command/configuration or setup failure; **2** one or more account checks
need attention or are unsupported. Partial status still prints all rows/JSON.

## Development and verification

`Cargo.toml` owns the release version; `Cargo.lock` locks dependencies. `shell.nix` pins
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
guards. Organization cost adapters additionally support bounded pagination and
public API-version headers; see [cost mappings](docs/cost-adapters.md). Signing,
other multi-step requests and nonstandard authentication need further capabilities. Built-in endpoint
support is not a guarantee every key/plan has the required permission.

See the [OpenAI](docs/openai.md), [Anthropic](docs/anthropic.md), and
[Google](docs/google.md) account guides for setup and provider-specific limitations.
