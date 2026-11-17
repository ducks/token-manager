# Token Manager

Read-only AI account management. Never route inference, buy credits, change billing,
create provider accounts or call paid inference endpoints.

- Provider endpoints/mappings belong in providers/*.toml; the engine has no
  provider-specific branches. Add behavioral fixture coverage for new mappings.
- Credentials belong in the OS keyring, environment, or explicitly referenced
  private files; never ordinary account config, cache, logs or test snapshots.
- Preserve unknown/missing/error states. Never represent unavailable data as zero.
- Use the pinned nix-shell and root Makefile. Keep network tests local and isolated.
- Do not use real accounts unless authorized. Never print keys or raw error bodies.
- No commits, merges, tags, publishing or pushing without explicit authorization.
