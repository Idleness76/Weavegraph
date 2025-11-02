#!/usr/bin/env bash
# ci-quick.sh - Fast pre-push checks (skip MSRV and optional tools)
# Usage: ./scripts/ci-quick.sh

set -e

echo "🚀 Running quick CI checks (stable toolchain only)..."
echo "======================================================"
echo ""

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

FAILED=0

run_check() {
    local name="$1"
    local cmd="$2"
    
    echo -e "${YELLOW}Running: $name${NC}"
    if eval "$cmd"; then
        echo -e "${GREEN}✓ $name passed${NC}"
        echo ""
    else
        echo -e "${RED}✗ $name failed${NC}"
        echo ""
        FAILED=1
    fi
}

# Essential checks that must pass
run_check "cargo fmt" "cargo fmt --all -- --check"
run_check "cargo clippy" "cargo clippy --workspace --all-targets -- -D warnings"
run_check "cargo test" "cargo test --workspace"
run_check "cargo doc" "RUSTDOCFLAGS='--cfg docsrs -D warnings' cargo doc --workspace --all-features --no-deps"

echo "======================================================"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✅ Quick checks passed!${NC}"
    echo ""
    echo "Note: Full CI also tests MSRV (1.89.0), deny, and machete."
    echo "Run ./scripts/ci-local.sh for complete validation."
    exit 0
else
    echo -e "${RED}❌ Some checks failed${NC}"
    exit 1
fi
