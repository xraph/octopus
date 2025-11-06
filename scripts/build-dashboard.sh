#!/bin/bash
set -e

echo "🐙 Building Octopus Admin Dashboard (Leptos + WASM)"
echo "===================================================="
echo ""

# Check if trunk is installed
if ! command -v trunk &> /dev/null; then
    echo "❌ Trunk is not installed!"
    echo "   Install with: cargo install trunk"
    exit 1
fi

# Check if wasm32-unknown-unknown target is installed
if ! rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
    echo "⚠️  Adding wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
fi

# Determine build mode
BUILD_MODE="${1:---release}"
if [ "$BUILD_MODE" = "--release" ]; then
    echo "🔨 Building in RELEASE mode (optimized)"
else
    echo "🔨 Building in DEBUG mode"
fi

echo ""

# Navigate to admin directory
cd "$(dirname "$0")/../crates/octopus-admin"

# Build with Trunk
echo "Building WASM bundle..."
trunk build $BUILD_MODE

echo ""
echo "✅ Build complete!"
echo ""
echo "📁 Output directory: crates/octopus-admin/dist/"
echo ""
echo "📦 Generated files:"
ls -lh dist/ | grep -E "\.(html|js|wasm)$" || ls -lh dist/

echo ""
echo "💡 Next steps:"
echo "   1. The gateway will embed these files at compile time"
echo "   2. Run: cargo build --bin octopus"
echo "   3. Access dashboard at: http://localhost:8080/admin"
echo ""

# Calculate total size
TOTAL_SIZE=$(du -sh dist/ | cut -f1)
echo "📊 Total bundle size: $TOTAL_SIZE"

