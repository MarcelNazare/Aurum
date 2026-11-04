CARGO   := cargo
ENV     := .env
ENV_EX  := .env.example
PORT    ?= 3000

.PHONY: help setup build release gui web web-headless clean check fmt lint

help: ## Show this help
	@echo "aurum-gui  · native desktop app"
	@echo "aurum-web  · web UI on http://127.0.0.1:$(PORT) (auto-opens browser)"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

setup: $(ENV) ## Create .env from .env.example
$(ENV):
	@cp $(ENV_EX) $(ENV)
	@echo "✔ created $(ENV) — paste your TWELVE_DATA_API_KEY"

build: ## Debug build (both binaries)
	$(CARGO) build

release: ## Optimized build (both binaries)
	$(CARGO) build --release
	@echo "✔ binaries: target/release/aurum-gui  and  target/release/aurum-web"

gui: $(ENV) ## Run the native desktop GUI
	$(CARGO) run --release --bin aurum-gui

web: $(ENV) ## Run the web UI (auto-opens browser)
	$(CARGO) run --release --bin aurum-web -- --port $(PORT)

web-headless: $(ENV) ## Run the web UI without opening a browser
	$(CARGO) run --release --bin aurum-web -- --port $(PORT) --no-open

check: ## Type-check all targets
	$(CARGO) check --all-targets

fmt: ## Format
	$(CARGO) fmt

lint: ## Clippy
	$(CARGO) clippy --all-targets -- -D warnings

clean: ## Clean build artifacts
	$(CARGO) clean