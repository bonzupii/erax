# erax Makefile

.PHONY: all build release install uninstall clean test fmt clippy check help

# Default target
all: release

# Build debug version
build:
	@echo "Building erax (debug)..."
	@cargo build

# Build optimized release version
release:
	@echo "Building erax (release)..."
	@cargo build --release
	@echo "Binary size:"
	@ls -lh target/release/erax | awk '{print $$5 " " $$9}'

# Install to /usr/local/bin (requires root)
install: release
	@echo "Installing erax to /usr/local/bin..."
	@install -m 755 target/release/erax /usr/local/bin/erax
	@echo "Done. Run 'erax --help' to get started."

# Uninstall
uninstall:
	@echo "Uninstalling erax..."
	@rm -f /usr/local/bin/erax
	@echo "Done."

# Clean build artifacts
clean:
	@echo "Cleaning..."
	@cargo clean

# Run tests
test:
	@cargo test --all

# Format code
fmt:
	@cargo fmt

# Run clippy
clippy:
	@cargo clippy -- -D warnings

# Pre-commit checks
check: fmt clippy test
	@echo "All checks passed."

# Help
help:
	@echo "erax Makefile"
	@echo ""
	@echo "Targets:"
	@echo "  make          - Build optimized release version"
	@echo "  make build    - Build debug version"
	@echo "  make release  - Build optimized release version"
	@echo "  make install  - Install to /usr/local/bin (requires sudo)"
	@echo "  make uninstall- Remove from /usr/local/bin"
	@echo "  make clean    - Remove build artifacts"
	@echo "  make test     - Run tests"
	@echo "  make fmt      - Format code"
	@echo "  make clippy   - Run lints"
	@echo "  make check    - fmt + clippy + test"

