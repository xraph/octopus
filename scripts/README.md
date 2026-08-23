# Development Scripts

Utility scripts for developing and running Octopus API Gateway.

## Available Scripts

### `dev.sh`

Main development script with various commands.

**Make executable:**
```bash
chmod +x scripts/dev.sh
```

**Usage:**
```bash
./scripts/dev.sh [command]
```

**Commands:**

- `build` - Build the entire project
- `test` - Run all tests (151 tests)
- `run` - Start the gateway server
- `example` - Run the quickstart example
- `check` - Run format, lint, and test checks
- `clean` - Clean build artifacts
- `docs` - Build and open documentation
- `help` - Show help message

**Examples:**

```bash
# Build the project
./scripts/dev.sh build

# Run all tests
./scripts/dev.sh test

# Start the gateway
./scripts/dev.sh run

# Run all checks (format, lint, test)
./scripts/dev.sh check

# Clean build artifacts
./scripts/dev.sh clean

# Open documentation in browser
./scripts/dev.sh docs
```

---

## Manual Commands

If you prefer to run commands directly:

### Build
```bash
cargo build --all-features
```

### Test
```bash
cargo test --all-features
```

### Run Gateway
```bash
cargo run --bin octopus -- serve --config config.example.yaml
```

### Run Example
```bash
cargo run --example quickstart
```

### Format Code
```bash
cargo fmt --all
```

### Lint Code
```bash
cargo clippy --all-features -- -D warnings
```

### Generate Documentation
```bash
cargo doc --all-features --no-deps --open
```

### Run Specific Crate Tests
```bash
cargo test -p octopus-core
cargo test -p octopus-router
cargo test -p octopus-auth
# etc.
```

---

## CI/CD

The project includes GitHub Actions workflows:

- **`.github/workflows/ci.yml`** - Continuous Integration
  - Runs on every push and PR
  - Tests on Ubuntu, macOS, Windows
  - Runs format, lint, security audit
  - Generates code coverage

- **`.github/workflows/release.yml`** - Release Automation
  - Triggers on version tags (v*)
  - Builds multi-platform binaries
  - Publishes Docker images
  - Creates GitHub releases

---

## Development Tips

### Watch Mode

For automatic rebuilds on file changes, use `cargo-watch`:

```bash
# Install cargo-watch
cargo install cargo-watch

# Watch and run tests
cargo watch -x test

# Watch and run specific crate tests
cargo watch -x 'test -p octopus-core'
```

### Fast Compilation

For faster development builds:

```bash
# Use the dev profile (default)
cargo build

# Skip tests when building
cargo build --all-features
```

### Debugging

Run with debug logging:

```bash
RUST_LOG=debug cargo run --bin octopus -- serve
```

Or trace level for maximum verbosity:

```bash
RUST_LOG=trace cargo run --bin octopus -- serve
```

---

## Troubleshooting

### "command not found: cargo"

Install Rust: https://rustup.rs/

### "permission denied"

Make scripts executable:

```bash
chmod +x scripts/*.sh
```

### Build Errors

Clean and rebuild:

```bash
cargo clean
cargo build --all-features
```

### Test Failures

Run tests with output:

```bash
cargo test --all-features -- --nocapture
```

---

## Additional Resources

- **Architecture**: `design/ARCHITECTURE.md`
- **Plugin Development**: `design/PLUGIN_SYSTEM.md`
- **FARP Integration**: `design/FARP_INTEGRATION.md`
- **Agent Guide**: `docs/AGENT_GUIDE.md`
- **Quickstart**: `QUICKSTART.md`


