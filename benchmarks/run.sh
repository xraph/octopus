#!/usr/bin/env bash
# ============================================================================
# Octopus vs Bastion — Gateway Benchmark Runner
# ============================================================================
#
# Usage:
#   ./benchmarks/run.sh              # Run all benchmarks
#   ./benchmarks/run.sh octopus      # Octopus only
#   ./benchmarks/run.sh bastion      # Bastion only
#   ./benchmarks/run.sh compare      # Side-by-side comparison
#
# Prerequisites:
#   - hey: go install github.com/rakyll/hey@latest
#   - jq: brew install jq
#   - Go 1.21+ and Rust toolchain
#
# ============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RESULTS_DIR="$SCRIPT_DIR/results"

# Ports
UPSTREAM_PORT=9999
OCTOPUS_PORT=8080
BASTION_PORT=8081

# Load test parameters
DURATION=10          # seconds per test
CONCURRENCY=50       # concurrent connections
REQUESTS=50000       # total requests

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

mkdir -p "$RESULTS_DIR"

# ── Helpers ──────────────────────────────────────────────────────────────────

log() { echo -e "${BLUE}[bench]${NC} $*"; }
ok()  { echo -e "${GREEN}[  ok ]${NC} $*"; }
err() { echo -e "${RED}[error]${NC} $*"; }
warn() { echo -e "${YELLOW}[warn ]${NC} $*"; }

check_deps() {
    local missing=0
    for cmd in hey jq go cargo; do
        if ! command -v "$cmd" &>/dev/null; then
            err "Missing dependency: $cmd"
            missing=1
        fi
    done
    if [ "$missing" -eq 1 ]; then
        echo ""
        echo "Install missing deps:"
        echo "  hey: go install github.com/rakyll/hey@latest"
        echo "  jq:  brew install jq"
        exit 1
    fi
    ok "All dependencies found"
}

wait_for_port() {
    local port=$1 name=$2 timeout=${3:-15}
    local start=$(date +%s)
    while ! curl -sf --max-time 2 "http://localhost:$port/" >/dev/null 2>&1; do
        if [ $(($(date +%s) - start)) -ge "$timeout" ]; then
            err "$name did not start on port $port within ${timeout}s"
            return 1
        fi
        sleep 0.5
    done
    ok "$name is ready on port $port"
}

kill_port() {
    local port=$1
    lsof -ti ":$port" 2>/dev/null | xargs -r kill -9 2>/dev/null || true
}

cleanup() {
    log "Cleaning up processes..."
    kill_port $OCTOPUS_PORT
    kill_port $BASTION_PORT
    # Kill upstream LAST (gateways depend on it)
    sleep 0.5
    kill_port $UPSTREAM_PORT
}
trap cleanup EXIT

# ── Start Upstream ───────────────────────────────────────────────────────────

start_upstream() {
    log "Starting mock upstream on :$UPSTREAM_PORT"
    kill_port $UPSTREAM_PORT
    cd "$SCRIPT_DIR/upstream"
    go run main.go -port "$UPSTREAM_PORT" > "$RESULTS_DIR/upstream.log" 2>&1 &
    cd "$ROOT_DIR"
    wait_for_port "$UPSTREAM_PORT" "Upstream"
}

# ── Start Octopus ────────────────────────────────────────────────────────────

start_octopus() {
    log "Building Octopus (release)..."
    cd "$ROOT_DIR"
    cargo build --release --bin octopus 2>/dev/null
    ok "Octopus built"

    kill_port $OCTOPUS_PORT
    sleep 0.5
    log "Starting Octopus on :$OCTOPUS_PORT"
    RUST_LOG=error "$ROOT_DIR/target/release/octopus" serve -c "$SCRIPT_DIR/octopus-bench.yaml" \
        > "$RESULTS_DIR/octopus.log" 2>&1 &
    sleep 1
    wait_for_port "$OCTOPUS_PORT" "Octopus" 20
}

stop_octopus() {
    kill_port $OCTOPUS_PORT
    sleep 0.5
}

# ── Start Bastion ────────────────────────────────────────────────────────────

start_bastion() {
    log "Building Bastion harness..."
    cd "$SCRIPT_DIR/bastion-harness"
    go build -o bastion-bench . 2>/dev/null || {
        warn "Bastion harness build failed — running go mod tidy first"
        go mod tidy 2>/dev/null
        go build -o bastion-bench . || {
            err "Failed to build Bastion harness"
            return 1
        }
    }
    ok "Bastion built"

    kill_port $BASTION_PORT
    log "Starting Bastion on :$BASTION_PORT"
    BASTION_PORT=$BASTION_PORT UPSTREAM_PORT=$UPSTREAM_PORT ./bastion-bench \
        > "$RESULTS_DIR/bastion.log" 2>&1 &
    cd "$ROOT_DIR"
    wait_for_port "$BASTION_PORT" "Bastion" 15
}

stop_bastion() {
    kill_port $BASTION_PORT
    sleep 0.5
}

# ── Run Load Test ────────────────────────────────────────────────────────────

run_hey() {
    local name=$1 url=$2 method=${3:-GET} body_file=${4:-}
    local out_file="$RESULTS_DIR/${name}.txt"

    log "  Running: $name ($method $url, ${CONCURRENCY}c, ${DURATION}s)"

    local hey_args=(
        -n "$REQUESTS"
        -c "$CONCURRENCY"
        -m "$method"
        -t 10
    )

    if [ -n "$body_file" ]; then
        hey_args+=(-D "$body_file" -T "application/json")
    fi

    hey "${hey_args[@]}" "$url" > "$out_file" 2>&1

    # Extract key metrics (hey uses %% in output)
    local rps lat_avg lat_p50 lat_p95 lat_p99
    rps=$(grep "Requests/sec:" "$out_file" | awk '{print $2}')
    lat_avg=$(grep "Average:" "$out_file" | head -1 | awk '{print $2}')
    lat_p50=$(grep "50%" "$out_file" | awk '{print $3}')
    lat_p95=$(grep "95%" "$out_file" | awk '{print $3}')
    lat_p99=$(grep "99%" "$out_file" | awk '{print $3}')

    echo -e "    ${CYAN}Req/s:${NC} ${BOLD}$rps${NC}  ${CYAN}Avg:${NC} ${lat_avg}s  ${CYAN}p50:${NC} ${lat_p50}s  ${CYAN}p95:${NC} ${lat_p95}s  ${CYAN}p99:${NC} ${lat_p99}s"

    # Save as JSON for comparison
    cat > "$RESULTS_DIR/${name}.json" <<EOF
{
  "name": "$name",
  "rps": $rps,
  "latency_avg": "$lat_avg",
  "latency_p50": "$lat_p50",
  "latency_p95": "$lat_p95",
  "latency_p99": "$lat_p99"
}
EOF
}

# ── Benchmark Scenarios ──────────────────────────────────────────────────────

# Create a 1KB JSON body for POST tests
create_test_body() {
    python3 -c "import json; print(json.dumps({'data': 'x'*900, 'id': 1}))" > "$RESULTS_DIR/body.json"
}

bench_gateway() {
    local gateway=$1 port=$2

    echo ""
    echo -e "${BOLD}━━━ Benchmarking ${gateway} (port ${port}) ━━━${NC}"
    echo ""

    # Scenario 1: Simple GET passthrough
    run_hey "${gateway}_get_passthrough" "http://localhost:$port/" "GET"

    # Scenario 2: POST with JSON body
    run_hey "${gateway}_post_json" "http://localhost:$port/echo" "POST" "$RESULTS_DIR/body.json"

    # Scenario 3: Large response (compression candidate)
    run_hey "${gateway}_large_response" "http://localhost:$port/large" "GET"

    echo ""
}

# ── Comparison Report ────────────────────────────────────────────────────────

print_comparison() {
    echo ""
    echo -e "${BOLD}╔══════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║           Octopus vs Bastion — Benchmark Comparison             ║${NC}"
    echo -e "${BOLD}╠══════════════════════════════════════════════════════════════════╣${NC}"
    echo ""

    printf "  %-25s %12s %12s %8s\n" "Scenario" "Octopus" "Bastion" "Winner"
    printf "  %-25s %12s %12s %8s\n" "─────────────────────────" "────────────" "────────────" "────────"

    for scenario in get_passthrough post_json large_response; do
        local oct_file="$RESULTS_DIR/octopus_${scenario}.json"
        local bas_file="$RESULTS_DIR/bastion_${scenario}.json"

        if [ -f "$oct_file" ] && [ -f "$bas_file" ]; then
            local oct_rps bas_rps winner
            oct_rps=$(jq -r '.rps' "$oct_file")
            bas_rps=$(jq -r '.rps' "$bas_file")

            if (( $(echo "$oct_rps > $bas_rps" | bc -l 2>/dev/null || echo 0) )); then
                winner="${GREEN}Octopus${NC}"
            else
                winner="${YELLOW}Bastion${NC}"
            fi

            local oct_p99 bas_p99
            oct_p99=$(jq -r '.latency_p99' "$oct_file")
            bas_p99=$(jq -r '.latency_p99' "$bas_file")

            printf "  %-25s %10s/s %10s/s %b\n" "$scenario (rps)" "$oct_rps" "$bas_rps" "$winner"
            printf "  %-25s %11ss %11ss\n" "  └─ p99 latency" "$oct_p99" "$bas_p99"
        fi
    done

    echo ""
    echo -e "${BOLD}╚══════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo "  Raw results: $RESULTS_DIR/"
    echo ""
}

# ── Concurrency scaling test ─────────────────────────────────────────────────

bench_concurrency_scaling() {
    local gateway=$1 port=$2

    echo -e "${BOLD}━━━ Concurrency Scaling: ${gateway} ━━━${NC}"

    for conc in 10 50 100 200 500; do
        CONCURRENCY=$conc
        local out_file="$RESULTS_DIR/${gateway}_scale_c${conc}.txt"
        hey -n 5000 -c "$conc" -t 10 "http://localhost:$port/" > "$out_file" 2>&1
        local rps=$(grep "Requests/sec:" "$out_file" | awk '{print $2}')
        local p99=$(grep "99%" "$out_file" | awk '{print $3}')
        printf "  c=%-4d  %10s req/s  p99=%ss\n" "$conc" "$rps" "$p99"
    done
    CONCURRENCY=50  # reset
    echo ""
}

# ── Main ─────────────────────────────────────────────────────────────────────

main() {
    local mode="${1:-all}"

    echo ""
    echo -e "${BOLD}🐙 Octopus vs Bastion — Gateway Benchmark Suite${NC}"
    echo ""

    check_deps
    create_test_body
    start_upstream

    case "$mode" in
        octopus)
            start_octopus
            bench_gateway "octopus" "$OCTOPUS_PORT"
            bench_concurrency_scaling "octopus" "$OCTOPUS_PORT"
            ;;
        bastion)
            start_bastion
            bench_gateway "bastion" "$BASTION_PORT"
            bench_concurrency_scaling "bastion" "$BASTION_PORT"
            ;;
        compare|all)
            start_octopus
            bench_gateway "octopus" "$OCTOPUS_PORT"
            bench_concurrency_scaling "octopus" "$OCTOPUS_PORT"
            stop_octopus

            start_bastion
            bench_gateway "bastion" "$BASTION_PORT"
            bench_concurrency_scaling "bastion" "$BASTION_PORT"
            stop_bastion

            print_comparison
            ;;
        *)
            echo "Usage: $0 [octopus|bastion|compare|all]"
            exit 1
            ;;
    esac

    ok "Benchmarks complete! Results in $RESULTS_DIR/"
}

main "$@"
