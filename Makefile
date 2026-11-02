# ═══════════════════════════════════════════════════════════════════════
# Aurum · XAU/USD ICT Scanner
# ═══════════════════════════════════════════════════════════════════════

BIN     := aurum
CARGO   := cargo
ENV     := .env
ENV_EX  := .env.example
INTERVAL ?= 15min

.PHONY: help setup build run release clean check fmt lint test watch tree

help: ## Show this help
	@echo "Aurum · XAU/USD ICT Scanner"
	@echo ""
	@echo "Usage: make <target> [INTERVAL=15min]"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

setup: $(ENV) ## Create .env from .env.example (if missing)

$(ENV):
	@if [ ! -f $(ENV_EX) ]; then \
		echo "✖ $(ENV_EX) not found — create it first"; exit 1; \
	fi
	@cp $(ENV_EX) $(ENV)
	@echo "✔ created $(ENV) — edit it and paste your TWELVE_DATA_API_KEY"
	@echo "  get a free key: https://twelvedata.com/register"

build: ## Compile in debug mode
	$(CARGO) build

release: ## Compile optimized release binary
	$(CARGO) build --release
	@echo "✔ binary: target/release/$(BIN)"

run: $(ENV) ## Run with defaults from .env
	$(CARGO) run --release

watch: $(ENV) ## Run with a custom interval (make watch INTERVAL=1h)
	$(CARGO) run --release -- $(INTERVAL)

check: ## Type-check without building binaries
	$(CARGO) check

fmt: ## Format source
	$(CARGO) fmt

lint: ## Run clippy linter
	$(CARGO) clippy -- -D warnings

test: ## Run tests (none yet, but ready)
	$(CARGO) test

tree: ## Show project layout
	@find . -type f \
		-not -path './target/*' \
		-not -path './.git/*' \
		| sort

clean: ## Remove build artifacts
	$(CARGO) clean
	@echo "✔ cleaned"