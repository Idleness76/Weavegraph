#!/usr/bin/env bash
# ci-quick.sh - Fast pre-push checks aligned to required CI settings
# Usage: ./scripts/ci-quick.sh

set -euo pipefail

REQUIRED_TOOLCHAIN="1.89.0"

echo "🚀 Running quick CI checks (toolchain: ${REQUIRED_TOOLCHAIN})..."
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

if ! command -v rustup >/dev/null 2>&1; then
    echo -e "${RED}rustup is required but not installed.${NC}"
    exit 1
fi

if ! rustup toolchain list | grep -q "${REQUIRED_TOOLCHAIN}"; then
    echo -e "${YELLOW}Installing Rust toolchain ${REQUIRED_TOOLCHAIN} (rustfmt, clippy)...${NC}"
    rustup toolchain install "${REQUIRED_TOOLCHAIN}" --profile minimal --component rustfmt --component clippy
fi

# Essential checks that must pass
run_check "cargo fmt" "cargo +${REQUIRED_TOOLCHAIN} fmt --all -- --check"
run_check "cargo clippy" "cargo +${REQUIRED_TOOLCHAIN} clippy --workspace --all-targets --all-features -- -D warnings"
run_check "cargo test" "cargo +${REQUIRED_TOOLCHAIN} test --workspace --all-features"
run_check "cargo doc" "RUSTDOCFLAGS='--cfg docsrs -D warnings' cargo +${REQUIRED_TOOLCHAIN} doc --workspace --all-features --no-deps"

echo "======================================================"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✅ Quick checks passed!${NC}"
    echo ""
    echo "Note: Full CI also runs cargo semver-checks, cargo deny, and cargo machete advisory."
    echo "Run ./scripts/ci-local.sh for complete validation."
    exit 0
else
    echo -e "${RED}❌ Some checks failed${NC}"
    exit 1
fi
