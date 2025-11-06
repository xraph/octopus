#!/bin/bash
# Development startup script for Octopus API Gateway

set -e

echo "🐙 Octopus API Gateway - Development Mode"
echo "========================================"
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Cargo not found. Please install Rust: https://rustup.rs/"
    exit 1
fi

echo -e "${GREEN}✓${NC} Rust/Cargo found"

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo "❌ Not in Octopus root directory. Please run from project root."
    exit 1
fi

echo -e "${GREEN}✓${NC} In Octopus project directory"
echo ""

# Parse command line arguments
COMMAND=${1:-help}

case $COMMAND in
    build)
        echo -e "${BLUE}Building Octopus...${NC}"
        cargo build --all-features
        echo ""
        echo -e "${GREEN}✓${NC} Build complete!"
        ;;
    
    test)
        echo -e "${BLUE}Running tests...${NC}"
        cargo test --all-features
        echo ""
        echo -e "${GREEN}✓${NC} All tests passed!"
        ;;
    
    run)
        echo -e "${BLUE}Starting Octopus gateway...${NC}"
        echo ""
        echo "📝 Using config: config.example.yaml"
        echo "🌐 Server will listen on: http://localhost:8080"
        echo "📊 Admin dashboard: http://localhost:8080/admin"
        echo ""
        cargo run --bin octopus -- serve --config config.example.yaml
        ;;
    
    example)
        echo -e "${BLUE}Running quickstart example...${NC}"
        echo ""
        cargo run --example quickstart
        ;;
    
    check)
        echo -e "${BLUE}Running checks...${NC}"
        echo ""
        echo "1. Format check..."
        cargo fmt --all -- --check
        echo -e "${GREEN}✓${NC} Formatting OK"
        echo ""
        echo "2. Clippy lints..."
        cargo clippy --all-features -- -D warnings
        echo -e "${GREEN}✓${NC} No clippy warnings"
        echo ""
        echo "3. Tests..."
        cargo test --all-features --quiet
        echo -e "${GREEN}✓${NC} All tests passing"
        echo ""
        echo -e "${GREEN}✓${NC} All checks passed!"
        ;;
    
    clean)
        echo -e "${YELLOW}Cleaning build artifacts...${NC}"
        cargo clean
        echo -e "${GREEN}✓${NC} Clean complete!"
        ;;
    
    docs)
        echo -e "${BLUE}Building and opening documentation...${NC}"
        cargo doc --all-features --no-deps --open
        ;;
    
    help|*)
        echo "Usage: ./scripts/dev.sh [command]"
        echo ""
        echo "Commands:"
        echo "  build     - Build the project"
        echo "  test      - Run all tests"
        echo "  run       - Start the gateway server"
        echo "  example   - Run the quickstart example"
        echo "  check     - Run format, lint, and test checks"
        echo "  clean     - Clean build artifacts"
        echo "  docs      - Build and open documentation"
        echo "  help      - Show this help message"
        echo ""
        echo "Examples:"
        echo "  ./scripts/dev.sh build"
        echo "  ./scripts/dev.sh test"
        echo "  ./scripts/dev.sh run"
        ;;
esac


