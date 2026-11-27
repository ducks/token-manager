# OpenAI accounts

`openai` and `chatgpt` are separate providers with different credentials and meters.

```sh
token-manager add openai --name openai-api --env OPENAI_ADMIN_KEY
token-manager add chatgpt --name personal --env CHATGPT_ACCESS_TOKEN
token-manager status openai-api --refresh
token-manager status personal --refresh
token-manager open openai-api billing
token-manager open personal billing
```

Use your existing secret-management workflow to supply environment variables; do
not put raw tokens in CLI arguments. Hidden keyring input and private token files
are also supported. A token file must contain only the token, not an auth.json
object. No credentials are discovered or imported automatically.

The API adapter requires an organization Admin key with Costs API access, not a
project inference key. It sums the last 30 completed UTC days and shows the exact
window. This is reported organization spend, not a prepaid balance. Empty reports
remain unavailable; explicit zero records remain zero. Pagination must complete:
errors never turn a partial page into a successful total. Reports use the existing
private cache and stale/error handling. No paid inference calls are made.

The experimental subscription adapter uses an explicitly supplied ChatGPT/Codex
OAuth access token to read the primary and secondary windows exposed by the usage
endpoint. Labels deliberately do not assume fixed window durations. Missing
windows remain unavailable. These meters do not describe every ChatGPT feature,
model-specific extra window, or API credit balance. Workspace-scoped tokens that
need an additional account-selection header are not supported in this version.
There is no OAuth login/refresh implementation; reconnect after the token expires.

Validation uses synthetic local HTTP fixtures. No live account has been verified.

Sources:
- https://developers.openai.com/api/reference/resources/admin/subresources/organization/subresources/usage/methods/costs
- https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs
- https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets_tests.rs
