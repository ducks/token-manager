CARGO ?= cargo
VERSION ?=
RELEASE_FLAGS ?=
.PHONY: help setup format lint test check build clean demo release-check release-archive release
help:
	@echo 'Token Manager — read-only AI accounts'
	@echo 'setup          Check the toolchain; enter nix-shell first'
	@echo 'format         Format Rust sources'
	@echo 'lint           Check formatting and Clippy'
	@echo 'test           Run isolated mapper, HTTP, credential-boundary and CLI tests'
	@echo 'check          Run the local quality gate'
	@echo 'build          Build the release binary with Cargo.lock'
	@echo 'clean          Remove only the owned target directory'
	@echo 'demo           Show synthetic Vercel/Go data via a local HTTP fixture'
	@echo 'release-check  Check/build and verify a crates.io publish dry run'
	@echo 'release-archive Prepare a local Linux archive without publishing'
	@echo 'release        Date-version, validate, commit, tag, push and publish to crates.io'
setup:
	$(CARGO) --version
	rustc --version
format:
	$(CARGO) fmt
lint:
	$(CARGO) fmt -- --check
	$(CARGO) clippy --locked --all-targets -- -D warnings
test:
	$(CARGO) test --locked
	python3 -m unittest discover -s tests -p 'release_workflow.py'
check: setup lint test
build:
	$(CARGO) build --locked --release
clean:
	python3 scripts/release.py clean
demo:
	$(CARGO) build --locked
	python3 scripts/demo.py
release-check: check build
	python3 scripts/release.py check $(RELEASE_FLAGS)
	$(CARGO) publish --locked --registry crates-io --dry-run $(RELEASE_FLAGS)
release-archive: check build
	python3 scripts/release.py prepare $(RELEASE_FLAGS)
release:
	python3 scripts/release.py release --cargo "$(CARGO)" --version "$(VERSION)" $(RELEASE_FLAGS)
