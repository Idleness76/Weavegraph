#!/usr/bin/env bash
# ci-local.sh - Run all required CI checks locally before pushing
# Usage: ./scripts/ci-local.sh

set -euo pipefail

REQUIRED_TOOLCHAIN="1.90.0"

echo "🔍 Running local CI checks (required toolchain: ${REQUIRED_TOOLCHAIN})..."
echo "=============================="
echo ""

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Track overall status
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

require_cmd() {
    local cmd="$1"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo -e "${RED}Missing required command: $cmd${NC}"
        FAILED=1
    fi
}

echo -e "${YELLOW}Verifying required tooling${NC}"
require_cmd rustup
require_cmd cargo
require_cmd cargo-semver-checks
require_cmd cargo-deny
if [ $FAILED -ne 0 ]; then
    echo -e "${RED}Install missing tools, then rerun.${NC}"
    exit 1
fi

if ! rustup toolchain list | grep -q "${REQUIRED_TOOLCHAIN}"; then
    echo -e "${YELLOW}Installing Rust toolchain ${REQUIRED_TOOLCHAIN} (rustfmt, clippy)...${NC}"
    rustup toolchain install "${REQUIRED_TOOLCHAIN}" --profile minimal --component rustfmt --component clippy
fi

# 1. Format check (fmt job)
run_check "cargo fmt (${REQUIRED_TOOLCHAIN})" "cargo +${REQUIRED_TOOLCHAIN} fmt --all -- --check"

# 2. Clippy on required toolchain (blocking)
run_check "cargo clippy (${REQUIRED_TOOLCHAIN})" "cargo +${REQUIRED_TOOLCHAIN} clippy --workspace --all-targets --all-features -- -D warnings"

# 3. Tests on required toolchain (blocking)
# Note: Integration tests with postgres require `docker-compose up postgres`.
# By default, run lib tests only (no external dependencies).
if pg_isready -h localhost -U weavegraph -d weavegraph_test >/dev/null 2>&1; then
    run_check "cargo test (${REQUIRED_TOOLCHAIN})" "cargo +${REQUIRED_TOOLCHAIN} test --workspace --all-features"
else
    echo -e "${YELLOW}⚠ Postgres not available at localhost:5432; running library tests only${NC}"
    echo -e "${YELLOW}  To run all tests: docker-compose up postgres && cargo +${REQUIRED_TOOLCHAIN} test --workspace --all-features${NC}"
    echo ""
    run_check "cargo test (${REQUIRED_TOOLCHAIN}, lib only)" "cargo +${REQUIRED_TOOLCHAIN} test --lib --all-features"
fi

# 4. Doc build (blocking)
run_check "cargo doc (${REQUIRED_TOOLCHAIN})" "RUSTDOCFLAGS='--cfg docsrs -D warnings' cargo +${REQUIRED_TOOLCHAIN} doc --workspace --all-features --no-deps"

# 5. Cargo semver-checks (blocking)
run_check "cargo semver-checks" "cargo semver-checks check-release --workspace"

# 6. Cargo deny (blocking)
run_check "cargo deny" "cargo deny check"

# 7. Cargo machete (advisory in CI)
if command -v cargo-machete &> /dev/null; then
    echo -e "${YELLOW}Running: cargo machete (advisory only)${NC}"
    if cargo machete --with-metadata; then
        echo -e "${GREEN}✓ cargo machete passed${NC}"
    else
        echo -e "${YELLOW}⚠ cargo machete found issues (non-blocking in CI)${NC}"
    fi
    echo ""
else
    echo -e "${YELLOW}⚠ Skipping cargo machete (not installed)${NC}"
    echo "  Install with: cargo install cargo-machete"
    echo ""
fi

# 8. Quick benchmark run (benchmarks job - validates benchmarks compile and run)
echo -e "${YELLOW}Running: benchmark compilation check${NC}"
if cargo +${REQUIRED_TOOLCHAIN} bench --workspace --no-run 2>/dev/null; then
    echo -e "${GREEN}✓ benchmarks compile successfully${NC}"
else
    echo -e "${YELLOW}⚠ benchmark compilation failed (non-blocking)${NC}"
fi
echo ""

echo "=============================="
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✅ All CI checks passed!${NC}"
    echo ""
    echo "You're ready to push. The CI pipeline should pass."
    exit 0
else
    echo -e "${RED}❌ Some checks failed${NC}"
    echo ""
    echo "Fix the issues above before pushing."
    exit 1
fi
