# Spike implementation and observed validation

Implemented a Linux Rust CLI in `~/dev/token-manager`, on the uncommitted
`spike/account-dashboard` branch. No remote, commit, tag or publication was created.

## Implemented

- `providers`, `add`, `login`, `status` and `open`; table and JSON output.
- Declarative provider TOML, JSON-pointer response mappings, exact decimal
  balances, optional quota windows and configurable authentication headers.
- Vercel AI Gateway, experimental OpenCode Go, a separate explicitly unsupported
  OpenCode Zen wallet entry, OpenRouter and DeepSeek mappings.
- Linux Secret Service client integration with hidden/stdin credential input;
  environment/private-file references; no credential value in account config/cache.
- Four concurrent bounded GET requests, HTTPS/origin restrictions, no redirects,
  response size limit, private cache and explicit stale/unknown/error states.
- Account edit locking, atomic private writes, definition binding before sending
  credentials, synthetic demo, pinned Nix shell, Make interface, CI and local
  binary-release preview.

`src/provider.rs` handles schema/mapping; `credentials.rs` resolves references;
`state.rs` owns local private files; `status.rs` handles HTTP/cache/normalization;
`main.rs` handles CLI interaction. Providers do not add Rust branches.

## Commands actually executed

| Command | Observed outcome |
|---|---|
| `nix-shell --run 'make check'` | PASS: formatting, Clippy with warnings denied, 10 integration tests |
| `make demo` under the pinned shell | PASS: real local HTTP requests, normalized Vercel/Go assertions, Zen unsupported |
| `make build` under the pinned shell | PASS: optimized Linux x86_64 binary |
| `make release RELEASE_FLAGS=--allow-dirty` | PASS: explicit local uncommitted preview, binary version, archive contents and SHA-256 |
| `python3 scripts/release.py check` | Expected FAIL: uncommitted source rejected; matching reason verified |
| Release binary `login vercel --key-file ...` then `status vercel --refresh --json` | PASS: real Vercel response, state `ready`, nonempty balance, not cached |
| `python3 -m py_compile scripts/release.py scripts/demo.py` | PASS |

Pinned shell observed Cargo 1.97.0 / Rust 1.97.1. Dependency versions are in
Cargo.lock. The real Vercel credential remains in its existing private file;
Token Manager stores only a reference. Account amounts, keys and private account
metadata are intentionally not included in this report or repository.

Eight HTTP/mapping/state tests cover decimal precision, exact subtraction,
per-currency rows, quota unknowns, separate Go/Zen products, cache reuse, stale
retention, HTTP 401/403/429/500, malformed responses, redirect refusal, secret
redaction, endpoint changes, custom definitions, permissions, locks and symlinks.
A real CLI lifecycle test covers adding, duplicate rejection, reconnecting,
unsupported Zen and JSON output. A separate keyring-client fixture verifies
stdin storage, lookup, HTTP authentication and absence of keys in ordinary state
or argument lists. Missing/unsupported data never becomes zero.

The initial Clippy pass found a redundant let-and-return expression; it was fixed
before the successful full gate. No tests were weakened to obtain a pass.

## Remaining / not claimed

- Real desktop keyring availability/unlock is unverified; its client protocol was
  tested with an isolated substitute. The live Vercel check uses a protected file.
- OpenCode Go is now live-verified as well as fixture-tested, but remains marked
  experimental because the usage endpoint is not a stable public billing contract.
  Zen has no claimed wallet API. Neither treats Go allowance as Zen money.
- No browser OAuth/device login, token refresh, TUI, removal/revocation command,
  account discovery, credit-expiry lot tracking or paginated/signed requests.
- Fresh cache is immediate; stale cache is retained on error. The CLI currently
  waits for a requested refresh rather than rendering an interactive live-update TUI.
- Remote CI, other platforms and standalone static binary distribution have not
  been verified. The Nix-built preview can depend on its local runtime closure.
- Provider definition changes conservatively require reconnect, even for a
  metadata-only edit. Old OS keyring entries are not automatically garbage-collected.
- There is no inference routing and no request that spends model tokens.

## Follow-up: live OpenCode Go check

The release binary was run with an isolated temporary configuration and an
environment reference populated only from OpenCode's `opencode-go` credential.
The distinct `opencode`/Zen entry was not substituted or sent to the endpoint.
`status opencode-go --refresh --json` exited 0 with state `ready`, live (not
cached) rolling/weekly/monthly usage percentages and reset timestamps. Temporary
account state was removed afterward; no key copy or account usage figures were
added to this repository. This validates real Go quota access, not Zen balance
access or absolute dollar/token allowance reporting. No code changes were needed.

## Follow-up: additional service adapters

On `feature/more-providers`, added Kimi/Moonshot international (USD) and China
(CNY), Featherless Developer spendable credit and experimental Synthetic request
quotas. Their TOML sources cite official APIs; new adapters have fixture coverage,
not live authentication/account verification. No new real account credentials were
looked up or sent. Existing Vercel binding/cache still reads successfully.

`build.rs` embeds sorted provider files automatically, replacing the manual Rust
list. The generic mapping engine now accepts absolute used/limit/unit quotas and
scalar response equality guards. Percent is computed with checked decimal
arithmetic; zero/missing limits are unknown rather than unlimited, and usage over
100% is retained. Moonshot's in-band error envelope cannot pass as fresh balance
information. Existing adapters serialize without added default fields, preserving
credential bindings.

Commands executed: `make check` (formatting, Clippy, **18 tests**), `make demo`,
`make build`, `python3 scripts/release.py check --allow-dirty` and
`git diff --check` under the pinned environment. The native binary includes nine
provider IDs. The new demo also exercises Synthetic's actual local HTTP request
and asserts its absolute 27/135 request count and derived 20% usage. No remote CI,
commits, releases or publications were performed; the earlier dist archive preview
was preserved rather than overwritten with different bytes.


## NanoGPT and Chutes adapter validation

Added three declarative adapters: NanoGPT USD wallet (empty account POST),
NanoGPT subscription counters (GET), and Chutes account balance (GET). The engine
supports declarative request methods, epoch reset conversion and boolean degraded
usage flags. Default GET/text fields and absent optional fields remain omitted
from serialization to preserve existing provider fingerprints.

Under the pinned `nix-shell`, `make check build` passed formatting, Clippy,
26 isolated tests and the locked release build. The new eight-test fixture batch
covers account separation, exact balances, quota units, timestamps, degraded
counters, null balances, private-profile exclusion, and the empty POST/auth-header
contract. No new real-account authentication is claimed. Public NanoGPT OpenAPI
and Chutes router source were reviewed on November 22, 2026.
