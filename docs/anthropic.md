# Anthropic accounts

`anthropic` and `claude` keep API organization spend separate from personal
subscription usage. They do not share credentials.

```sh
token-manager add anthropic --name api-work --env ANTHROPIC_ADMIN_KEY
token-manager add claude --name personal --env CLAUDE_ACCESS_TOKEN
token-manager status api-work --refresh
token-manager status personal --refresh
token-manager open api-work billing
token-manager open personal billing
```

Supply credentials through your existing secret-management workflow, hidden
keyring input, or a private file containing only the token. Raw secrets do not
belong in CLI arguments. No Claude Code auth files are automatically read or changed.

The API adapter uses a Claude Console Admin API key with the beta Usage and Cost
Analytics API. It sums cost records for the last 30 completed UTC days, converts
reported cents to USD with decimal arithmetic, and follows pagination. This is
organization spend, not remaining credits or a personal subscription allowance.
The API may lag up to 24 hours; commitment fees and some separately invoiced costs
are not included. Empty responses remain unavailable, and incomplete/malformed
pages fail without publishing partial totals. Existing private caches retain old
data with an explicit stale status if refresh fails.

The experimental Claude subscription adapter reads session, weekly, and weekly
Sonnet windows from the internal OAuth usage endpoint. It requires an explicitly
supplied OAuth access token with usage access, not an API/Admin key. There is no
OAuth sign-in or refresh implementation; reconnect when your token expires. This
endpoint may change or rate-limit requests. Additional model-specific windows and
extra-usage charges are not yet mapped. Missing windows remain unavailable rather
than appearing as unused allowances.

Validation uses synthetic local fixtures; neither adapter has been live-verified
with a real account. Both perform read-only GET requests.

Sources:
- https://platform.claude.com/docs/en/manage-claude/usage-cost-analytics-api
- https://github.com/steipete/CodexBar/blob/main/docs/claude.md
- https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work
