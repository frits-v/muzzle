.PHONY: build test test-unit test-integration release install deploy clean lint lint-sh fmt check

# Default target
all: check test build

# Development build (fast, with debug info)
build:
	cargo build

# Run all tests
test: test-unit

# Unit tests only
test-unit:
	cargo test

# Integration tests (requires git repos in /tmp)
test-integration:
	cargo test --test integration

# Release build (optimized, stripped)
release:
	cargo build --release

# Install release binaries to bin/
install: release
	@mkdir -p bin
	@for b in session-start permissions changelog session-end ensure-worktree; do \
		cp target/release/$$b bin/$$b; \
		echo "  installed bin/$$b"; \
	done

# Deploy release binaries to target directory.
# Override with: make deploy DEPLOY_TARGET=/path/to/hooks
DEPLOY_TARGET ?= $(HOME)/.local/share/muzzle

deploy: release
	@if [ -n "$$(git status --porcelain -- src/ Cargo.toml Cargo.lock Makefile)" ]; then \
		echo "ERROR: Uncommitted changes in tracked build files."; \
		echo "Commit or stash before deploying."; \
		git status --short -- src/ Cargo.toml Cargo.lock Makefile; \
		exit 1; \
	fi
	@echo "Deploying to $(DEPLOY_TARGET)/"
	@mkdir -p $(DEPLOY_TARGET)/bin $(DEPLOY_TARGET)/src
	@# Binaries
	@for b in session-start permissions changelog session-end ensure-worktree; do \
		cp target/release/$$b $(DEPLOY_TARGET)/bin/$$b; \
		echo "  bin/$$b"; \
	done
	@# Source + build files (for future builds in-place)
	@rsync -a --delete --exclude='target/' --exclude='.git/' --exclude='.agents/' \
		src/ $(DEPLOY_TARGET)/src/
	@cp Cargo.toml Cargo.lock $(DEPLOY_TARGET)/ 2>/dev/null || cp Cargo.toml $(DEPLOY_TARGET)/
	@echo "Deployed to $(DEPLOY_TARGET)/"

# Lint Rust
lint:
	cargo clippy -- -D warnings

# Lint shell scripts (shellcheck + shfmt)
lint-sh:
	shellcheck scripts/*.sh
	shfmt -d -i 2 -ci -bn scripts/*.sh

# Format check
fmt:
	cargo fmt -- --check

# Format fix
fmt-fix:
	cargo fmt

# Type check without building
check:
	cargo check

# Clean build artifacts
clean:
	cargo clean

# Show binary sizes after release build
sizes: release
	@echo "Binary sizes:"
	@ls -lh target/release/session-start target/release/permissions \
		target/release/changelog target/release/session-end \
		target/release/ensure-worktree 2>/dev/null | \
		awk '{print "  " $$NF ": " $$5}'

# Run a single test by name
test-one:
	@test -n "$(NAME)" || (echo "Usage: make test-one NAME=test_name" && exit 1)
	cargo test $(NAME) -- --nocapture
