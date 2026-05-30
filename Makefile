# Octopus API Gateway - Makefile
# Production-ready build automation
#
# Usage:
#   make help           - Show all available commands
#   make build          - Build debug version
#   make release        - Build optimized release
#   make test           - Run all tests
#   make docker         - Build Docker image
#
# Requirements:
#   - Rust 1.75+
#   - Docker (for container builds)
#   - cargo-nextest (optional, for faster tests)

.PHONY: help
.DEFAULT_GOAL := help

# Configuration
CARGO := cargo
DOCKER := docker
PROJECT_NAME := octopus
REGISTRY := ghcr.io/xraph
VERSION := $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
COMMIT := $(shell git rev-parse --short HEAD 2>/dev/null || echo "unknown")
BUILD_DATE := $(shell date -u +"%Y-%m-%dT%H:%M:%SZ")

# Build flags
CARGO_FLAGS :=
CARGO_RELEASE_FLAGS := --release
CARGO_ALL_FEATURES := --all-features
CARGO_WORKSPACE := --workspace

# Colors for output
COLOR_RESET := \033[0m
COLOR_BOLD := \033[1m
COLOR_GREEN := \033[32m
COLOR_YELLOW := \033[33m
COLOR_BLUE := \033[34m
COLOR_CYAN := \033[36m

# ==============================================================================
# Help
# ==============================================================================

help: ## Show this help message
	@echo "$(COLOR_BOLD)🐙 Octopus API Gateway - Build System$(COLOR_RESET)"
	@echo ""
	@echo "$(COLOR_CYAN)Available targets:$(COLOR_RESET)"
	@awk 'BEGIN {FS = ":.*##"; printf ""} /^[a-zA-Z_-]+:.*?##/ { printf "  $(COLOR_GREEN)%-20s$(COLOR_RESET) %s\n", $$1, $$2 } /^##@/ { printf "\n$(COLOR_BOLD)%s$(COLOR_RESET)\n", substr($$0, 5) } ' $(MAKEFILE_LIST)
	@echo ""
	@echo "$(COLOR_YELLOW)Examples:$(COLOR_RESET)"
	@echo "  make build          # Build debug version"
	@echo "  make test           # Run all tests"
	@echo "  make lint           # Check code quality"
	@echo "  make release        # Build optimized release"
	@echo "  make docker         # Build Docker image"
	@echo ""

# ==============================================================================
##@ 🔨 Build
# ==============================================================================

.PHONY: build build-release build-cli build-plugins build-all check

build: ## Build debug version (fast, for development)
	@echo "$(COLOR_BLUE)Building $(PROJECT_NAME) (debug)...$(COLOR_RESET)"
	$(CARGO) build $(CARGO_ALL_FEATURES)
	@echo "$(COLOR_GREEN)✓ Build complete$(COLOR_RESET)"

build-release: release ## Alias for 'release'

release: ## Build optimized release version
	@echo "$(COLOR_BLUE)Building $(PROJECT_NAME) (release)...$(COLOR_RESET)"
	$(CARGO) build $(CARGO_RELEASE_FLAGS) $(CARGO_ALL_FEATURES)
	@echo "$(COLOR_GREEN)✓ Release build complete: target/release/$(PROJECT_NAME)$(COLOR_RESET)"

build-cli: ## Build CLI binary only
	@echo "$(COLOR_BLUE)Building octopus-cli...$(COLOR_RESET)"
	$(CARGO) build --bin octopus $(CARGO_ALL_FEATURES)

build-plugins: ## Build all plugin libraries
	@echo "$(COLOR_BLUE)Building plugins...$(COLOR_RESET)"
	@cd plugins/auth-jwt && $(CARGO) build --release
	@cd plugins/rate-limiter && $(CARGO) build --release
	@cd plugins/cache-redis && $(CARGO) build --release
	@cd plugins/kafka-producer && $(CARGO) build --release
	@echo "$(COLOR_GREEN)✓ All plugins built$(COLOR_RESET)"

build-all: build build-plugins ## Build everything (main binary + plugins)

check: ## Fast compile check without producing binaries
	@echo "$(COLOR_BLUE)Running cargo check...$(COLOR_RESET)"
	$(CARGO) check $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES)

# ==============================================================================
##@ 🧪 Testing
# ==============================================================================

.PHONY: test test-unit test-integration test-all test-coverage bench
.PHONY: test-chaos test-chaos-setup test-chaos-teardown test-chaos-network test-chaos-upstream test-chaos-resource

test: ## Run all tests
	@echo "$(COLOR_BLUE)Running tests...$(COLOR_RESET)"
	$(CARGO) test $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES)

test-unit: ## Run unit tests only
	@echo "$(COLOR_BLUE)Running unit tests...$(COLOR_RESET)"
	$(CARGO) test $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) --lib

test-integration: ## Run integration tests only
	@echo "$(COLOR_BLUE)Running integration tests...$(COLOR_RESET)"
	$(CARGO) test $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) --test '*'

test-integration-proxy: ## Run octopus-proxy integration tests only
	@echo "$(COLOR_BLUE)Running octopus-proxy integration tests...$(COLOR_RESET)"
	$(CARGO) test --package octopus-proxy --test integration

test-chaos-setup: ## Start chaos testing infrastructure (Docker + Toxiproxy)
	@echo "$(COLOR_BLUE)Starting chaos testing infrastructure...$(COLOR_RESET)"
	@cd crates/octopus-proxy/tests/chaos && ./setup.sh
	@echo "$(COLOR_GREEN)✓ Chaos infrastructure ready$(COLOR_RESET)"
	@echo "$(COLOR_YELLOW)Toxiproxy API: http://localhost:8474$(COLOR_RESET)"
	@echo "$(COLOR_YELLOW)Mock upstreams: http://localhost:20000-20002$(COLOR_RESET)"

test-chaos-teardown: ## Stop chaos testing infrastructure
	@echo "$(COLOR_BLUE)Stopping chaos testing infrastructure...$(COLOR_RESET)"
	@cd crates/octopus-proxy/tests/chaos && docker compose down
	@echo "$(COLOR_GREEN)✓ Chaos infrastructure stopped$(COLOR_RESET)"

test-chaos: test-chaos-setup ## Run all chaos tests (requires Docker)
	@echo "$(COLOR_BLUE)Running chaos tests...$(COLOR_RESET)"
	$(CARGO) test --package octopus-proxy-chaos-tests -- --ignored
	@echo "$(COLOR_GREEN)✓ Chaos tests complete$(COLOR_RESET)"

test-chaos-network: test-chaos-setup ## Run network failure chaos tests
	@echo "$(COLOR_BLUE)Running network failure chaos tests...$(COLOR_RESET)"
	$(CARGO) test --package octopus-proxy-chaos-tests test_network -- --ignored

test-chaos-upstream: test-chaos-setup ## Run upstream failure chaos tests
	@echo "$(COLOR_BLUE)Running upstream failure chaos tests...$(COLOR_RESET)"
	$(CARGO) test --package octopus-proxy-chaos-tests test_upstream -- --ignored

test-chaos-resource: test-chaos-setup ## Run resource limit chaos tests
	@echo "$(COLOR_BLUE)Running resource limit chaos tests...$(COLOR_RESET)"
	$(CARGO) test --package octopus-proxy-chaos-tests test_resource -- --ignored

test-chaos-cascading: test-chaos-setup ## Run cascading failure chaos tests
	@echo "$(COLOR_BLUE)Running cascading failure chaos tests...$(COLOR_RESET)"
	$(CARGO) test --package octopus-proxy-chaos-tests test_cascading -- --ignored

test-nextest: ## Run tests with nextest (faster, install: cargo install nextest)
	@echo "$(COLOR_BLUE)Running tests with nextest...$(COLOR_RESET)"
	$(CARGO) nextest run $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES)

test-coverage: ## Generate test coverage report (requires cargo-tarpaulin)
	@echo "$(COLOR_BLUE)Generating coverage report...$(COLOR_RESET)"
	$(CARGO) tarpaulin --out Html --output-dir coverage $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES)
	@echo "$(COLOR_GREEN)✓ Coverage report: coverage/index.html$(COLOR_RESET)"

test-all: test ## Alias for 'test'

bench: ## Run benchmarks
	@echo "$(COLOR_BLUE)Running benchmarks...$(COLOR_RESET)"
	$(CARGO) bench $(CARGO_ALL_FEATURES)

# ==============================================================================
##@ 📝 Code Quality
# ==============================================================================

.PHONY: lint fmt clippy audit deny fix pre-commit

lint: fmt clippy ## Run all linters (fmt + clippy)

fmt: ## Check code formatting
	@echo "$(COLOR_BLUE)Checking formatting...$(COLOR_RESET)"
	$(CARGO) fmt --all -- --check

fmt-fix: ## Fix code formatting
	@echo "$(COLOR_BLUE)Formatting code...$(COLOR_RESET)"
	$(CARGO) fmt --all

clippy: ## Run clippy linter
	@echo "$(COLOR_BLUE)Running clippy...$(COLOR_RESET)"
	$(CARGO) clippy $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) -- -D warnings

clippy-fix: ## Fix clippy warnings automatically
	@echo "$(COLOR_BLUE)Fixing clippy warnings...$(COLOR_RESET)"
	$(CARGO) clippy $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) --fix --allow-dirty

audit: ## Check for security vulnerabilities
	@echo "$(COLOR_BLUE)Running security audit...$(COLOR_RESET)"
	$(CARGO) audit

deny: ## Check dependencies with cargo-deny
	@echo "$(COLOR_BLUE)Checking dependencies...$(COLOR_RESET)"
	$(CARGO) deny check

fix: fmt-fix clippy-fix ## Auto-fix formatting and clippy issues

pre-commit: fmt clippy test ## Run all checks before committing (recommended)
	@echo "$(COLOR_GREEN)✓ All pre-commit checks passed!$(COLOR_RESET)"

# ==============================================================================
##@ 🚀 Run & Development
# ==============================================================================

.PHONY: run run-release dev watch example

run: ## Run the gateway (debug mode)
	@echo "$(COLOR_BLUE)Starting Octopus Gateway (debug)...$(COLOR_RESET)"
	$(CARGO) run --bin octopus -- serve --config config.example.yaml

run-release: ## Run the gateway (release mode)
	@echo "$(COLOR_BLUE)Starting Octopus Gateway (release)...$(COLOR_RESET)"
	$(CARGO) run --release --bin octopus -- serve --config config.example.yaml

dev: ## Development mode with auto-reload (requires cargo-watch)
	@echo "$(COLOR_BLUE)Starting development server with auto-reload...$(COLOR_RESET)"
	$(CARGO) watch -x 'run --bin octopus -- serve --config config.example.yaml'

watch: dev ## Alias for 'dev'

example: example-quickstart ## Run the quickstart example (alias)

example-quickstart: ## Run the quickstart example
	@echo "$(COLOR_BLUE)Running quickstart example...$(COLOR_RESET)"
	cd examples && $(CARGO) run --bin quickstart || echo "$(COLOR_YELLOW)Note: Run 'cd examples && cargo build' first if needed$(COLOR_RESET)"

example-mdns: ## Run the mDNS service example
	@echo "$(COLOR_BLUE)Running mDNS service example...$(COLOR_RESET)"
	@echo "$(COLOR_YELLOW)Service will register as 'example-service' on port 8080$(COLOR_RESET)"
	@echo "$(COLOR_YELLOW)Press Ctrl+C to stop$(COLOR_RESET)"
	@echo ""
	cd examples && $(CARGO) run --bin mdns_service

example-mdns-custom: ## Run mDNS service with custom settings (usage: make example-mdns-custom NAME=myservice PORT=9000)
	@echo "$(COLOR_BLUE)Running mDNS service with custom settings...$(COLOR_RESET)"
	cd examples && $(CARGO) run --bin mdns_service -- --name $(NAME) --port $(PORT)

examples-build: ## Build all examples
	@echo "$(COLOR_BLUE)Building examples...$(COLOR_RESET)"
	cd examples && $(CARGO) build --all-features
	@echo "$(COLOR_GREEN)✓ Examples built$(COLOR_RESET)"

examples-list: ## List all available examples
	@echo "$(COLOR_CYAN)Available examples:$(COLOR_RESET)"
	@echo ""
	@echo "  $(COLOR_GREEN)mdns_service$(COLOR_RESET)   - mDNS-enabled HTTP service (examples/mdns_service.rs)"
	@echo "  $(COLOR_GREEN)quickstart$(COLOR_RESET)     - Gateway configuration demo (examples/quickstart.rs)"
	@echo ""
	@echo "$(COLOR_YELLOW)Run with:$(COLOR_RESET)"
	@echo "  make example           # Run quickstart"
	@echo "  make example-mdns      # Run mDNS service"
	@echo "  make examples-build    # Build all examples"
	@echo ""
	@echo "$(COLOR_YELLOW)Or directly:$(COLOR_RESET)"
	@echo "  cd examples && cargo run --bin mdns_service"
	@echo ""

# ==============================================================================
##@ 🐳 Docker
# ==============================================================================

.PHONY: docker docker-build docker-run docker-push docker-buildx docker-release docker-clean docker-compose-up docker-compose-down

PLATFORMS ?= linux/amd64,linux/arm64

docker: docker-build ## Build Docker image

docker-build: ## Build Docker image
	@echo "$(COLOR_BLUE)Building Docker image...$(COLOR_RESET)"
	$(DOCKER) build -t $(PROJECT_NAME):$(VERSION) \
		--build-arg VERSION=$(VERSION) \
		--build-arg COMMIT=$(COMMIT) \
		--build-arg BUILD_DATE=$(BUILD_DATE) \
		.
	$(DOCKER) tag $(PROJECT_NAME):$(VERSION) $(PROJECT_NAME):latest
	@echo "$(COLOR_GREEN)✓ Docker image built: $(PROJECT_NAME):$(VERSION)$(COLOR_RESET)"

docker-run: ## Run Docker container locally
	@echo "$(COLOR_BLUE)Starting Docker container...$(COLOR_RESET)"
	$(DOCKER) run --rm -it \
		-p 8080:8080 \
		-p 9090:9090 \
		-v $(PWD)/config.example.yaml:/etc/octopus/config.yaml \
		$(PROJECT_NAME):latest

docker-push: ## Push Docker image to registry
	@echo "$(COLOR_BLUE)Pushing to $(REGISTRY)/$(PROJECT_NAME):$(VERSION)...$(COLOR_RESET)"
	$(DOCKER) tag $(PROJECT_NAME):$(VERSION) $(REGISTRY)/$(PROJECT_NAME):$(VERSION)
	$(DOCKER) tag $(PROJECT_NAME):$(VERSION) $(REGISTRY)/$(PROJECT_NAME):latest
	$(DOCKER) push $(REGISTRY)/$(PROJECT_NAME):$(VERSION)
	$(DOCKER) push $(REGISTRY)/$(PROJECT_NAME):latest

docker-buildx: ## Build multi-arch image locally (no push) — mirrors CI platforms
	@echo "$(COLOR_BLUE)Building multi-arch image ($(PLATFORMS))...$(COLOR_RESET)"
	$(DOCKER) buildx build \
		--platform $(PLATFORMS) \
		--build-arg VERSION=$(VERSION) \
		--build-arg COMMIT=$(COMMIT) \
		--build-arg BUILD_DATE=$(BUILD_DATE) \
		-t $(REGISTRY)/$(PROJECT_NAME):$(VERSION) \
		.

docker-release: ## Build & push multi-arch image to $(REGISTRY) (mirrors docker-release.yml)
	@echo "$(COLOR_BLUE)Releasing multi-arch image to $(REGISTRY)/$(PROJECT_NAME):$(VERSION)...$(COLOR_RESET)"
	$(DOCKER) buildx build \
		--platform $(PLATFORMS) \
		--build-arg VERSION=$(VERSION) \
		--build-arg COMMIT=$(COMMIT) \
		--build-arg BUILD_DATE=$(BUILD_DATE) \
		-t $(REGISTRY)/$(PROJECT_NAME):$(VERSION) \
		-t $(REGISTRY)/$(PROJECT_NAME):latest \
		--push \
		.
	@echo "$(COLOR_GREEN)✓ Pushed $(REGISTRY)/$(PROJECT_NAME):$(VERSION)$(COLOR_RESET)"

docker-clean: ## Remove Docker images
	@echo "$(COLOR_BLUE)Cleaning Docker images...$(COLOR_RESET)"
	$(DOCKER) rmi $(PROJECT_NAME):$(VERSION) $(PROJECT_NAME):latest || true

docker-compose-up: ## Start services with docker-compose
	docker-compose up -d

docker-compose-down: ## Stop services with docker-compose
	docker-compose down

# ==============================================================================
##@ 📚 Documentation
# ==============================================================================

.PHONY: docs docs-open docs-check

docs: ## Build documentation
	@echo "$(COLOR_BLUE)Building documentation...$(COLOR_RESET)"
	$(CARGO) doc $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) --no-deps

docs-open: ## Build and open documentation in browser
	@echo "$(COLOR_BLUE)Building and opening documentation...$(COLOR_RESET)"
	$(CARGO) doc $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) --no-deps --open

docs-check: ## Check documentation for errors
	@echo "$(COLOR_BLUE)Checking documentation...$(COLOR_RESET)"
	$(CARGO) doc $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) --no-deps --document-private-items

# ==============================================================================
##@ 🧹 Cleanup
# ==============================================================================

.PHONY: clean clean-all clean-plugins clean-docker clean-target distclean

clean: ## Clean build artifacts
	@echo "$(COLOR_BLUE)Cleaning build artifacts...$(COLOR_RESET)"
	$(CARGO) clean
	@echo "$(COLOR_GREEN)✓ Clean complete$(COLOR_RESET)"

clean-plugins: ## Clean plugin build artifacts
	@echo "$(COLOR_BLUE)Cleaning plugin artifacts...$(COLOR_RESET)"
	@cd plugins/auth-jwt && $(CARGO) clean
	@cd plugins/rate-limiter && $(CARGO) clean
	@cd plugins/cache-redis && $(CARGO) clean
	@cd plugins/kafka-producer && $(CARGO) clean

clean-docker: docker-clean ## Clean Docker artifacts

clean-target: ## Remove target directory
	@echo "$(COLOR_BLUE)Removing target directory...$(COLOR_RESET)"
	rm -rf target

clean-all: clean clean-plugins clean-docker ## Deep clean everything
	@echo "$(COLOR_GREEN)✓ Deep clean complete$(COLOR_RESET)"

distclean: clean-all ## Complete cleanup including dependencies
	rm -rf target Cargo.lock

# ==============================================================================
##@ 🔧 Installation & Deployment
# ==============================================================================

.PHONY: install uninstall install-tools

install: release ## Install the binary to ~/.cargo/bin
	@echo "$(COLOR_BLUE)Installing $(PROJECT_NAME)...$(COLOR_RESET)"
	$(CARGO) install --path octopus-cli
	@echo "$(COLOR_GREEN)✓ Installed to ~/.cargo/bin/$(PROJECT_NAME)$(COLOR_RESET)"

uninstall: ## Uninstall the binary
	@echo "$(COLOR_BLUE)Uninstalling $(PROJECT_NAME)...$(COLOR_RESET)"
	$(CARGO) uninstall $(PROJECT_NAME)

install-tools: ## Install development tools
	@echo "$(COLOR_BLUE)Installing development tools...$(COLOR_RESET)"
	$(CARGO) install cargo-watch cargo-nextest cargo-audit cargo-deny cargo-tarpaulin
	@echo "$(COLOR_GREEN)✓ Development tools installed$(COLOR_RESET)"

# ==============================================================================
##@ 📊 CI/CD Helpers
# ==============================================================================

.PHONY: ci ci-test ci-lint ci-build verify

ci: ci-lint ci-test ci-build ## Run all CI checks

ci-lint: ## CI: Lint checks
	@echo "$(COLOR_BLUE)[CI] Running lint checks...$(COLOR_RESET)"
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) -- -D warnings

ci-test: ## CI: Run tests with coverage
	@echo "$(COLOR_BLUE)[CI] Running tests...$(COLOR_RESET)"
	$(CARGO) test $(CARGO_WORKSPACE) $(CARGO_ALL_FEATURES) -- --nocapture

ci-build: ## CI: Build release
	@echo "$(COLOR_BLUE)[CI] Building release...$(COLOR_RESET)"
	$(CARGO) build --release $(CARGO_ALL_FEATURES)

verify: pre-commit ## Verify project before push (lint + test + build)
	@echo "$(COLOR_GREEN)✓ Project verified and ready!$(COLOR_RESET)"

# ==============================================================================
##@ 🔍 Diagnostics
# ==============================================================================

.PHONY: version tree outdated bloat size

version: ## Show version information
	@echo "Project:    $(PROJECT_NAME)"
	@echo "Version:    $(VERSION)"
	@echo "Commit:     $(COMMIT)"
	@echo "Build Date: $(BUILD_DATE)"
	@echo ""
	@rustc --version
	@$(CARGO) --version

tree: ## Show dependency tree
	$(CARGO) tree $(CARGO_ALL_FEATURES)

outdated: ## Check for outdated dependencies
	$(CARGO) outdated

bloat: ## Analyze binary size (requires cargo-bloat)
	$(CARGO) bloat --release --bin $(PROJECT_NAME)

size: release ## Show binary size
	@ls -lh target/release/$(PROJECT_NAME) | awk '{print "Binary size: " $$5}'

# ==============================================================================
##@ 🎯 Quick Commands
# ==============================================================================

.PHONY: all fast full

all: build test ## Build and test

fast: check ## Fast check without full build

full: clean release test lint docs docker ## Full build pipeline
	@echo "$(COLOR_GREEN)✓ Full build pipeline complete!$(COLOR_RESET)"

# ==============================================================================
##@ 📊 Benchmarks
# ==============================================================================

.PHONY: bench bench-octopus bench-bastion bench-compare

bench: bench-compare ## Run all benchmarks (Octopus vs Bastion)

bench-octopus: ## Benchmark Octopus only
	@./benchmarks/run.sh octopus

bench-bastion: ## Benchmark Bastion only
	@./benchmarks/run.sh bastion

bench-compare: ## Side-by-side Octopus vs Bastion comparison
	@./benchmarks/run.sh compare
