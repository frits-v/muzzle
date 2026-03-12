.PHONY: build test test-unit test-integration release install clean lint fmt check

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

# Lint
lint:
	cargo clippy -- -D warnings

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
