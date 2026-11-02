CARGO   := cargo
ENV     := .env
ENV_EX  := .env.example
PORT    ?= 3000

.PHONY: help setup build release tui web clean check fmt lint

help: ## Show this help
	@echo "aurum-tui  · terminal scanner"
	@echo "aurum-web  · web UI on http://127.0.0.1:$(PORT)"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

setup: $(ENV) ## Create .env from .env.example
$(ENV):
	@cp $(ENV_EX) $(ENV)
	@echo "✔ created $(ENV) — paste your TWELVE_DATA_API_KEY"

build: ## Debug build (both binaries)
	$(CARGO) build

release: ## Optimized build (both binaries)
	$(CARGO) build --release
	@echo "✔ binaries: target/release/aurum-tui  and  target/release/aurum-web"

tui: $(ENV) ## Run the terminal scanner
	$(CARGO) run --release --bin aurum-tui

web: $(ENV) ## Run the web UI on http://127.0.0.1:$(PORT)
	$(CARGO) run --release --bin aurum-web -- --port $(PORT)

check: ## Type-check everything
	$(CARGO) check --all-targets

fmt: ## Format
	$(CARGO) fmt

lint: ## Clippy
	$(CARGO) clippy --all-targets -- -D warnings

clean: ## Clean build artifacts
	$(CARGO) clean