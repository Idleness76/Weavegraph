# =============================================================================
# Weavegraph - Makefile
# =============================================================================

.PHONY: help lint test ci-quick ci-local all

# Default target
.DEFAULT_GOAL := help

help: ## Show this help message
	@echo "Available targets:"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'
	@echo ""

all: ## Run lint and test
	$(MAKE) lint
	$(MAKE) test

lint: ## Check code formatting and run clippy
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings

test: ## Run all tests (nextest + doctests)
	cargo nextest run --workspace --all-features
	cargo test --doc

ci-quick: ## Run quick CI checks script
	./scripts/ci-quick.sh

ci-local: ## Run local CI checks script
	./scripts/ci-local.sh
