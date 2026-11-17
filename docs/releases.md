# Releases

Cargo.toml is authoritative; Cargo.lock and `--version` must agree. Use Semantic
Versioning and `vVERSION` tags if publication is later authorized. No release has
been committed, tagged or published. CHANGELOG.md records only actual changes.

```sh
nix-shell
make check
make build
make release-check
make release
```

Release-check rejects dirty/uncommitted source by default and existing release
tags. During this uncommitted spike, `RELEASE_FLAGS=--allow-dirty` explicitly
permits a LOCAL PREVIEW; it is not evidence of a committed release.

Release creates a native Linux x86_64 binary archive with README/LICENSE and a
SHA-256 file in dist/. It validates the binary version and expected archive
contents. Identical existing bytes are a no-op; conflicting files and symlinks
are preserved and rejected. No credential or user config is packaged.

Nix-built binaries may reference the local Nix runtime closure. This preview is
verified on this Linux x86_64 host, not claimed as a portable static binary,
cross-platform matrix, signed artifact or reproducible compiler output. Source
formatting/metadata checks do not establish bit-for-bit compilation reproducibility.

Tests/build failures stop preparation. Retry after fixing the cause; no remote
state changed. If an artifact conflicts, inspect it rather than silently replacing
it. A published package/tag could require a corrected new version rather than
withdrawal; external publication and recovery are not implemented here.

`make clean` invokes Cargo for this repository's target directory only, refusing
symlinks, external target overrides and unexpected top-level contents. It never
cleans user configuration, credentials or dist/ archives. Do not put source/user
data inside Cargo's owned target directory.
