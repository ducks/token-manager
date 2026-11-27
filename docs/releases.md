# Releases

Cargo.toml is authoritative; Cargo.lock and `--version` must agree. Releases use
`YYYYMMDD.0.PATCH`, starting at patch zero each day and incrementing for further
same-day tags. `make release VERSION=20261122.0.1` can select an explicit date
version. Record actual changes under CHANGELOG.md’s Unreleased section. The repository is https://github.com/ducks/token-manager.

## Validate without publishing

```sh
nix-shell
make release-check
```

This runs formatting, Clippy, Rust and release-workflow tests, the locked release
build, local metadata checks, and `cargo publish --locked --registry crates-io
--dry-run`. Cargo packages the source and builds the packaged crate without
uploading it. Registry/dependency access may require network connectivity.

Release checks reject dirty source and mismatched binary/lock versions. An
existing version tag must point at HEAD, allowing CI to verify a tagged release. For an uncommitted preview only:

```sh
make release-check RELEASE_FLAGS=--allow-dirty
```

Dirty previews do not authorize publication. `make release` rejects preview
flags and requires clean `main` synchronized with `origin/main`.

## Publish to crates.io

After reviewing, committing, merging and pushing the intended changes to main,
authenticate as a crates.io account authorized to publish this crate, using
`cargo login` or an appropriate `CARGO_REGISTRY_TOKEN` in your private environment.
Never put a token in the Makefile, repository, command arguments or release
notes. Name availability and ownership are enforced by crates.io.

```sh
make release
```

**This command commits, tags, pushes and publishes.** It fetches tags, chooses
the date version, creates `release/vVERSION`, updates Cargo.toml/Cargo.lock and
moves Unreleased changelog entries into that version. After committing those
changes it runs `make release-check`, merges with `--no-ff` into main, creates
an annotated `vVERSION` tag, atomically pushes main and that tag, then runs
`cargo publish --locked --registry crates-io`. GitHub Releases and deployments
are not created. Agents must not invoke it without release authorization.

The `.crate` source archive under `target/package/` is distinct from the native
binary archive. A dry run verifies packaging/buildability, not ownership,
credentials, bit-for-bit compiler reproducibility, signing or provenance.

## Local Linux binary archive

```sh
make release-archive
# Or an uncommitted local preview:
make release-archive RELEASE_FLAGS=--allow-dirty
```

This never invokes Cargo publish. It creates a native Linux x86_64 archive with both `token-manager` and `tm`, plus
README/LICENSE and a SHA-256 file in dist/. Binary version and archive contents
are checked. Identical existing bytes are a no-op; conflicting files and symlinks
are retained and rejected for manual inspection. No credentials or user config
are packaged. Nix-built binaries may require the local Nix runtime closure;
these archives are not claimed to be portable static builds.

## Failure and recovery

Tests, build, metadata or dry-run failures stop before tags, pushes or registry
uploads. A failed check may leave a local release branch and version commit for
inspection. Fix and validate it with `make release-check`; do not blindly delete
work or rerun `make release` from a non-main branch. Authentication or ownership failures require fixing the
credential or choosing an available crate name; do not guess another endpoint.

If upload fails or times out, check crates.io for this exact name/version before
retrying: the registry may have accepted it despite a lost response. Published
versions cannot be overwritten. For a bad release, publish a corrected new
version and consider yanking the old version; yanking is not deletion and does
not undo installations. A failed atomic Git push prevents registry publication. If publication fails
after the Git push, preserve the release tag and inspect crates.io before
retrying `cargo publish --locked --registry crates-io` from that clean tagged
commit. A transient upload failure does not justify a different tag or deleting
release evidence. Release creation refuses an existing local or remote tag.

`make clean` removes only Cargo's owned target directory. It refuses symlinks,
external target overrides and unexpected top-level contents, and preserves user
configuration, credentials and dist/ archives.
