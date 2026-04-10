#!/bin/bash
# Setup script for chaos testing infrastructure
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/docker-compose.yml"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    log_error "Docker is not running. Please start Docker first."
    exit 1
fi

# Check if docker-compose is available
if ! command -v docker-compose &> /dev/null; then
    log_error "docker-compose not found. Please install docker-compose."
    exit 1
fi

# Start services
log_info "Starting chaos testing infrastructure..."
cd "$SCRIPT_DIR"
docker-compose -f "$COMPOSE_FILE" up -d

# Wait for services to be healthy
log_info "Waiting for services to be healthy..."
sleep 5

# Check Toxiproxy
if curl -sf http://localhost:8474/version > /dev/null; then
    log_info "✓ Toxiproxy is running"
else
    log_error "✗ Toxiproxy is not responding"
    exit 1
fi

# Check mock upstreams
for port in 80; do
    if docker exec octopus-mock-1 wget -q -O- http://localhost:$port/health > /dev/null 2>&1; then
        log_info "✓ Mock upstream 1 is healthy"
    else
        log_warn "✗ Mock upstream 1 is not healthy"
    fi
done

# Create Toxiproxy proxies
log_info "Creating Toxiproxy proxies..."

# Proxy for mock-upstream-1
curl -sf -X POST http://localhost:8474/proxies \
    -H "Content-Type: application/json" \
    -d '{
        "name": "mock-upstream-1",
        "listen": "0.0.0.0:20000",
        "upstream": "mock-upstream-1:80",
        "enabled": true
    }' > /dev/null && log_info "✓ Created proxy for mock-upstream-1" || log_warn "Proxy may already exist"

# Proxy for mock-upstream-2
curl -sf -X POST http://localhost:8474/proxies \
    -H "Content-Type: application/json" \
    -d '{
        "name": "mock-upstream-2",
        "listen": "0.0.0.0:20001",
        "upstream": "mock-upstream-2:80",
        "enabled": true
    }' > /dev/null && log_info "✓ Created proxy for mock-upstream-2" || log_warn "Proxy may already exist"

# Proxy for mock-upstream-3
curl -sf -X POST http://localhost:8474/proxies \
    -H "Content-Type: application/json" \
    -d '{
        "name": "mock-upstream-3",
        "listen": "0.0.0.0:20002",
        "upstream": "mock-upstream-3:80",
        "enabled": true
    }' > /dev/null && log_info "✓ Created proxy for mock-upstream-3" || log_warn "Proxy may already exist"

log_info ""
log_info "======================================"
log_info "Chaos testing infrastructure is ready!"
log_info "======================================"
log_info ""
log_info "Services:"
log_info "  - Toxiproxy API:     http://localhost:8474"
log_info "  - Mock upstream 1:   http://localhost:20000"
log_info "  - Mock upstream 2:   http://localhost:20001"
log_info "  - Mock upstream 3:   http://localhost:20002"
log_info ""
log_info "To stop: docker-compose -f $COMPOSE_FILE down"
log_info "To view logs: docker-compose -f $COMPOSE_FILE logs -f"
