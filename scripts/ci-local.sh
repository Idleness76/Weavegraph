#!/usr/bin/env bash
# ci-local.sh - Run all CI checks locally before pushing
# Usage: ./scripts/ci-local.sh

set -e  # Exit on first error

echo "🔍 Running local CI checks..."
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

# 1. Format check (fmt job)
run_check "cargo fmt" "cargo fmt --all -- --check"

# 2. Clippy on MSRV (clippy job - 1.89.0)
# Note: Only run if you have rustup and want to test MSRV locally
if command -v rustup &> /dev/null; then
    if rustup toolchain list | grep -q "1.89.0"; then
        run_check "cargo clippy (MSRV 1.89.0)" "cargo +1.89.0 clippy --workspace --all-targets -- -D warnings"
    else
        echo -e "${YELLOW}⚠ Skipping MSRV clippy (1.89.0 not installed)${NC}"
        echo "  Install with: rustup toolchain install 1.89.0"
        echo ""
    fi
fi

# 3. Clippy on stable (clippy job - stable)
run_check "cargo clippy (stable)" "cargo clippy --workspace --all-targets -- -D warnings"

# 4. Tests on MSRV (test job - 1.89.0)
if command -v rustup &> /dev/null; then
    if rustup toolchain list | grep -q "1.89.0"; then
        run_check "cargo test (MSRV 1.89.0)" "cargo +1.89.0 test --workspace"
    else
        echo -e "${YELLOW}⚠ Skipping MSRV tests (1.89.0 not installed)${NC}"
        echo ""
    fi
fi

# 5. Tests on stable (test job - stable)
run_check "cargo test (stable)" "cargo test --workspace"

# 6. Doc build (doc job)
run_check "cargo doc" "RUSTDOCFLAGS='--cfg docsrs -D warnings' cargo doc --workspace --all-features --no-deps"

# 7. Cargo deny (deny job)
if command -v cargo-deny &> /dev/null; then
    run_check "cargo deny" "cargo deny check"
else
    echo -e "${YELLOW}⚠ Skipping cargo deny (not installed)${NC}"
    echo "  Install with: cargo install cargo-deny"
    echo ""
fi

# 8. Cargo machete (machete job - continue-on-error in CI)
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
