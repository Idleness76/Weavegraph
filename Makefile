SHELL := /bin/bash

CARGO ?= cargo
SQLX ?= sqlx
DATABASE_URL ?= sqlite://$(PWD)/data/weavegraph.db
OLLAMA_VOLUME_NAME ?= ollama_data

.PHONY: fmt fmt-check clippy test deny doc machete semver-checks bench migrate migrate-revert migrate-status \
	check_setup check_ollama_volume make_ollama_volume weavegraph

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test:
	$(CARGO) test --workspace

deny:
	$(CARGO) deny check

doc:
	$(CARGO) doc --workspace --all-features --no-deps

machete:
	@if ! $(CARGO) --list | grep -q 'machete'; then \
		echo "Installing cargo-machete..."; \
		$(CARGO) install cargo-machete --locked; \
	fi
	$(CARGO) machete --with-metadata

semver-checks:
	@if ! $(CARGO) --list | grep -q 'semver-checks'; then \
		echo "Installing cargo-semver-checks..."; \
		$(CARGO) install cargo-semver-checks --locked; \
	fi
	$(CARGO) semver-checks check-release --package weavegraph

bench:
	$(CARGO) bench --workspace

bench-quick:
	$(CARGO) bench --workspace -- --quick

migrate:
	$(SQLX) migrate run --source weavegraph/migrations --database-url "$(DATABASE_URL)"

migrate-revert:
	$(SQLX) migrate revert --source weavegraph/migrations --database-url "$(DATABASE_URL)"

migrate-status:
	$(SQLX) migrate info --source weavegraph/migrations --database-url "$(DATABASE_URL)"

check_setup:
	make check_ollama_volume

check_ollama_volume:
	@if docker volume ls | grep -q $(OLLAMA_VOLUME_NAME) ; then \
                echo "Volume exists"; \
            else \
                make make_ollama_volume; \
            fi

make_ollama_volume:
	#create the volume from the base image
	docker create -v $(OLLAMA_VOLUME_NAME):/data --name $(OLLAMA_VOLUME_NAME) alpine:latest

weavegraph:check_setup
	docker compose -f docker-compose.yml up -d --build --pull always
