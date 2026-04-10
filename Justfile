# Octopus API Gateway - Justfile
# Modern task runner for Rust projects
#
# Installation: cargo install just
# Usage: just <recipe>
#
# Quick start:
#   just build      - Build debug version
#   just test       - Run tests
#   just run        - Start the gateway
#   just --list     - Show all recipes

# Configuration
project_name := "octopus"
registry := "ghcr.io/xraph"

# Get version from git or use "dev"
version := `git describe --tags --always --dirty 2>/dev/null || echo "dev"`
commit := `git rev-parse --short HEAD 2>/dev/null || echo "unknown"`
build_date := `date -u +"%Y-%m-%dT%H:%M:%SZ"`

# Default recipe (shown when you run 'just' without arguments)
default:
    @just --list

# ==============================================================================
# 🔨 Build Recipes
# ==============================================================================

# Build debug version (fast, for development)
build:
    @echo "🔨 Building {{project_name}} (debug)..."
    cargo build --all-features
    @echo "✓ Build complete"

# Build optimized release version
release:
    @echo "🔨 Building {{project_name}} (release)..."
    cargo build --release --all-features
    @echo "✓ Release build: target/release/{{project_name}}"

# Build CLI binary only
build-cli:
    @echo "🔨 Building octopus-cli..."
    cargo build --bin octopus --all-features

# Build all plugin libraries
build-plugins:
    @echo "🔨 Building plugins..."
    cd plugins/auth-jwt && cargo build --release
    cd plugins/rate-limiter && cargo build --release
    cd plugins/cache-redis && cargo build --release
    cd plugins/kafka-producer && cargo build --release
    @echo "✓ All plugins built"

# Build everything (main binary + plugins)
build-all: build build-plugins

# Fast compile check without producing binaries
check:
    @echo "🔍 Running cargo check..."
    cargo check --workspace --all-features

# ==============================================================================
# 🧪 Testing Recipes
# ==============================================================================

# Run all tests
test:
    @echo "🧪 Running tests..."
    cargo test --workspace --all-features

# Run tests with output
test-verbose:
    @echo "🧪 Running tests (verbose)..."
    cargo test --workspace --all-features -- --nocapture

# Run unit tests only
test-unit:
    @echo "🧪 Running unit tests..."
    cargo test --workspace --all-features --lib

# Run integration tests only
test-integration:
    @echo "🧪 Running integration tests..."
    cargo test --workspace --all-features --test '*'

# Run tests with nextest (faster, requires: cargo install nextest)
test-nextest:
    @echo "🧪 Running tests with nextest..."
    cargo nextest run --workspace --all-features

# Generate test coverage report (requires: cargo install cargo-tarpaulin)
coverage:
    @echo "📊 Generating coverage report..."
    cargo tarpaulin --out Html --output-dir coverage --workspace --all-features
    @echo "✓ Coverage report: coverage/index.html"

# Run benchmarks
bench:
    @echo "⚡ Running benchmarks..."
    cargo bench --all-features

# ==============================================================================
# 📝 Code Quality Recipes
# ==============================================================================

# Run all linters (fmt + clippy)
lint: fmt clippy

# Check code formatting
fmt:
    @echo "📝 Checking formatting..."
    cargo fmt --all -- --check

# Fix code formatting
fmt-fix:
    @echo "📝 Formatting code..."
    cargo fmt --all

# Run clippy linter
clippy:
    @echo "📎 Running clippy..."
    cargo clippy --workspace --all-features -- -D warnings

# Fix clippy warnings automatically
clippy-fix:
    @echo "📎 Fixing clippy warnings..."
    cargo clippy --workspace --all-features --fix --allow-dirty

# Check for security vulnerabilities
audit:
    @echo "🔒 Running security audit..."
    cargo audit

# Check dependencies with cargo-deny
deny:
    @echo "🔍 Checking dependencies..."
    cargo deny check

# Auto-fix formatting and clippy issues
fix: fmt-fix clippy-fix

# Run all checks before committing
pre-commit: fmt clippy test
    @echo "✓ All pre-commit checks passed!"

# ==============================================================================
# 🚀 Run & Development Recipes
# ==============================================================================

# Run the gateway (debug mode)
run:
    @echo "🚀 Starting Octopus Gateway (debug)..."
    cargo run --bin octopus -- serve --config config.example.yaml

# Run the gateway (release mode)
run-release:
    @echo "🚀 Starting Octopus Gateway (release)..."
    cargo run --release --bin octopus -- serve --config config.example.yaml

# Development mode with auto-reload (requires: cargo install cargo-watch)
dev:
    @echo "👀 Starting development server with auto-reload..."
    cargo watch -x 'run --bin octopus -- serve --config config.example.yaml'

# Run with debug logging
run-debug:
    @echo "🐛 Starting with debug logging..."
    RUST_LOG=debug cargo run --bin octopus -- serve --config config.example.yaml

# Run the quickstart example
example: example-quickstart

# Run the quickstart example
example-quickstart:
    @echo "📘 Running quickstart example..."
    cd examples && cargo run --bin quickstart

# Run the mDNS service example (default: port 8080)
example-mdns:
    @echo "📡 Running mDNS service example..."
    @echo "   Service will register as 'example-service' on port 8080"
    @echo "   Press Ctrl+C to stop"
    @echo ""
    cd examples && cargo run --bin mdns_service

# Run mDNS service with custom settings
example-mdns-custom name port:
    @echo "📡 Running mDNS service: {{name}} on port {{port}}..."
    cd examples && cargo run --bin mdns_service -- --name {{name}} --port {{port}}

# Build all examples
examples-build:
    @echo "🔨 Building examples..."
    cd examples && cargo build --all-features
    @echo "✓ Examples built"

# List all available examples
examples-list:
    @echo "📚 Available examples:"
    @echo ""
    @echo "  • mdns_service   - mDNS-enabled HTTP service"
    @echo "  • quickstart     - Gateway configuration demo"
    @echo ""
    @echo "Run with:"
    @echo "  just example               # Run quickstart"
    @echo "  just example-mdns          # Run mDNS service"
    @echo "  just example-mdns-custom my-service 9000"
    @echo "  just examples-build        # Build all examples"
    @echo ""
    @echo "Or directly:"
    @echo "  cd examples && cargo run --bin mdns_service"
    @echo ""

# ==============================================================================
# 🐳 Docker Recipes
# ==============================================================================

# Build Docker image
docker-build:
    @echo "🐳 Building Docker image..."
    docker build -t {{project_name}}:{{version}} \
        --build-arg VERSION={{version}} \
        --build-arg COMMIT={{commit}} \
        --build-arg BUILD_DATE={{build_date}} \
        .
    docker tag {{project_name}}:{{version}} {{project_name}}:latest
    @echo "✓ Docker image built: {{project_name}}:{{version}}"

# Run Docker container locally
docker-run:
    @echo "🐳 Starting Docker container..."
    docker run --rm -it \
        -p 8080:8080 \
        -p 9090:9090 \
        -v {{justfile_directory()}}/config.example.yaml:/etc/octopus/config.yaml \
        {{project_name}}:latest

# Push Docker image to registry
docker-push:
    @echo "🐳 Pushing to {{registry}}/{{project_name}}:{{version}}..."
    docker tag {{project_name}}:{{version}} {{registry}}/{{project_name}}:{{version}}
    docker tag {{project_name}}:{{version}} {{registry}}/{{project_name}}:latest
    docker push {{registry}}/{{project_name}}:{{version}}
    docker push {{registry}}/{{project_name}}:latest

# Remove Docker images
docker-clean:
    @echo "🧹 Cleaning Docker images..."
    -docker rmi {{project_name}}:{{version}} {{project_name}}:latest

# Build and run Docker container
docker: docker-build docker-run

# Start services with docker-compose
compose-up:
    docker-compose up -d

# Stop services with docker-compose
compose-down:
    docker-compose down

# ==============================================================================
# 📚 Documentation Recipes
# ==============================================================================

# Build documentation
docs:
    @echo "📚 Building documentation..."
    cargo doc --workspace --all-features --no-deps

# Build and open documentation in browser
docs-open:
    @echo "📚 Building and opening documentation..."
    cargo doc --workspace --all-features --no-deps --open

# Check documentation for errors
docs-check:
    @echo "📚 Checking documentation..."
    cargo doc --workspace --all-features --no-deps --document-private-items

# ==============================================================================
# 🧹 Cleanup Recipes
# ==============================================================================

# Clean build artifacts
clean:
    @echo "🧹 Cleaning build artifacts..."
    cargo clean
    @echo "✓ Clean complete"

# Clean plugin build artifacts
clean-plugins:
    @echo "🧹 Cleaning plugin artifacts..."
    cd plugins/auth-jwt && cargo clean
    cd plugins/rate-limiter && cargo clean
    cd plugins/cache-redis && cargo clean
    cd plugins/kafka-producer && cargo clean

# Remove target directory
clean-target:
    @echo "🧹 Removing target directory..."
    rm -rf target

# Deep clean everything
clean-all: clean clean-plugins docker-clean
    @echo "✓ Deep clean complete"

# Complete cleanup including dependencies
distclean: clean-all
    rm -rf target Cargo.lock

# ==============================================================================
# 🔧 Installation & Setup Recipes
# ==============================================================================

# Install the binary to ~/.cargo/bin
install: release
    @echo "🔧 Installing {{project_name}}..."
    cargo install --path octopus-cli
    @echo "✓ Installed to ~/.cargo/bin/{{project_name}}"

# Uninstall the binary
uninstall:
    @echo "🔧 Uninstalling {{project_name}}..."
    cargo uninstall {{project_name}}

# Install development tools
install-tools:
    @echo "🔧 Installing development tools..."
    cargo install cargo-watch cargo-nextest cargo-audit cargo-deny cargo-tarpaulin cargo-bloat cargo-outdated just
    @echo "✓ Development tools installed"

# Setup development environment
setup: install-tools
    @echo "🔧 Setting up development environment..."
    @echo "✓ Development environment ready!"

# ==============================================================================
# 📊 CI/CD Recipes
# ==============================================================================

# Run all CI checks
ci: ci-lint ci-test ci-build

# CI: Lint checks
ci-lint:
    @echo "🔍 [CI] Running lint checks..."
    cargo fmt --all -- --check
    cargo clippy --workspace --all-features -- -D warnings

# CI: Run tests
ci-test:
    @echo "🧪 [CI] Running tests..."
    cargo test --workspace --all-features

# CI: Build release
ci-build:
    @echo "🔨 [CI] Building release..."
    cargo build --release --all-features

# Verify project before push (comprehensive check)
verify: pre-commit
    @echo "✅ Project verified and ready!"

# ==============================================================================
# 🔍 Diagnostic Recipes
# ==============================================================================

# Show version information
version:
    @echo "Project:    {{project_name}}"
    @echo "Version:    {{version}}"
    @echo "Commit:     {{commit}}"
    @echo "Build Date: {{build_date}}"
    @echo ""
    @rustc --version
    @cargo --version

# Show dependency tree
tree:
    cargo tree --all-features

# Check for outdated dependencies (requires: cargo install cargo-outdated)
outdated:
    cargo outdated

# Analyze binary size (requires: cargo install cargo-bloat)
bloat: release
    cargo bloat --release --bin {{project_name}}

# Show binary size
size: release
    @ls -lh target/release/{{project_name}} | awk '{print "Binary size: " $$5}'

# Show project statistics
stats:
    @echo "📊 Project Statistics:"
    @echo ""
    @echo "Lines of code:"
    @find crates -name "*.rs" | xargs wc -l | tail -1
    @echo ""
    @echo "Number of crates:"
    @ls -1 crates | wc -l
    @echo ""
    @echo "Number of plugins:"
    @ls -1 plugins | wc -l

# ==============================================================================
# 🎯 Quick Command Recipes
# ==============================================================================

# Build and test
all: build test

# Fast check without full build
fast: check

# Full build pipeline (comprehensive)
full: clean release test lint docs docker-build
    @echo "✅ Full build pipeline complete!"

# Quick development cycle: format, check, test
quick: fmt-fix check test-unit
    @echo "✅ Quick check complete!"

# ==============================================================================
# 🔧 Utility Recipes
# ==============================================================================

# Update dependencies
update:
    @echo "⬆️  Updating dependencies..."
    cargo update

# Update dependencies to latest compatible versions
upgrade:
    @echo "⬆️  Upgrading dependencies..."
    cargo upgrade

# Generate Cargo.lock
lock:
    @echo "🔒 Generating Cargo.lock..."
    cargo generate-lockfile

# Vendor dependencies
vendor:
    @echo "📦 Vendoring dependencies..."
    cargo vendor

# Print this help
help:
    @just --list --unsorted
