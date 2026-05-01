.PHONY: help build build-release build-macos run test test-all install clean \
        release release-patch release-minor bump-formula wait-for-release-assets smoke-test \
        bench-all bench-startup bench-playback bench-pause bench-memory bench-cpu \
        bench-profile bench-watch bench-results bench-analyze bench-clean

BINARY     := looper
INSTALL_DIR := /usr/local/bin
FIXTURE    := tests/fixtures/sound.mp3
TAP_REPO   := https://github.com/program247365/homebrew-tap.git
TAP_DIR    := /tmp/homebrew-tap-update
VERSION    := $(shell cargo metadata --no-deps --format-version 1 | python3 -c "import sys,json;print(json.load(sys.stdin)['packages'][0]['version'])")

help: ## Show available targets
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make \033[36m<target>\033[0m\n\nTargets:\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

# ── Local development ─────────────────────────────────────────────────────────

build: ## Build debug binary
	cargo build

build-release: ## Build optimized release binary
	cargo build --release

build-macos: ## Build release binary for x86_64 macOS
	cargo build --target=x86_64-apple-darwin --release

run: ## Play the fixture audio file on loop (Ctrl+C to stop)
	cargo run -- play --url $(FIXTURE)

test: ## Run non-interactive tests
	cargo test

test-all: ## Run all tests including those requiring audio output
	cargo test -- --ignored

install: build-release ## Install release binary to $(INSTALL_DIR)
	sudo install -m 755 target/release/$(BINARY) $(INSTALL_DIR)/$(BINARY)
	@find target/release/build -name '*.dylib' | while read DYLIB_SRC; do \
		DYLIB_NAME=$$(basename $$DYLIB_SRC); \
		DYLIB_OLD=$$(otool -L $(INSTALL_DIR)/$(BINARY) | awk -v n="$$DYLIB_NAME" '$$0 ~ n {print $$1}'); \
		if [ -n "$$DYLIB_OLD" ]; then \
			sudo install -m 644 $$DYLIB_SRC /usr/local/lib/$$DYLIB_NAME; \
			sudo install_name_tool -change $$DYLIB_OLD /usr/local/lib/$$DYLIB_NAME $(INSTALL_DIR)/$(BINARY); \
		fi; \
	done

clean: ## Remove build artifacts
	cargo clean

# ── Performance Benchmarking ──────────────────────────────────────────────────
# Usage:
#   make bench-all        — Run all benchmarks
#   make bench-startup    — Measure startup time and initial memory
#   make bench-playback   — Measure memory/CPU during playback
#   make bench-pause      — Measure pause behavior
#   make bench-memory     — Deep memory profiling
#   make bench-cpu        — CPU profiling
#   make bench-profile    — Full profiling session (100s+)
#   make bench-watch      — Real-time memory monitoring
#   make bench-results    — Show latest results
#   make bench-analyze    — Analyze results and identify memory hogs
#   make bench-clean      — Clean benchmark results

BENCH_DIR := bench
BENCH_SCRIPTS := $(BENCH_DIR)/scripts
BENCH_RESULTS := $(BENCH_DIR)/results

bench-setup:
	@mkdir -p $(BENCH_RESULTS)
	@$(MAKE) build-release

bench-all: bench-setup ## Run all performance benchmarks
	@echo "=== Running Full Benchmark Suite ==="
	@$(MAKE) bench-startup
	@$(MAKE) bench-playback
	@$(MAKE) bench-pause
	@$(MAKE) bench-memory
	@$(MAKE) bench-cpu
	@echo ""
	@echo "=== Benchmark Complete ==="
	@$(MAKE) bench-results

bench-startup: bench-setup ## Measure startup performance
	@$(BENCH_SCRIPTS)/bench-startup.sh

bench-playback: bench-setup ## Measure playback performance (30s)
	@$(BENCH_SCRIPTS)/bench-playback.sh

bench-pause: bench-setup ## Measure pause behavior
	@$(BENCH_SCRIPTS)/bench-pause.sh

bench-memory: bench-setup ## Deep memory profiling (2min)
	@$(BENCH_SCRIPTS)/memory-profile.sh

bench-cpu: bench-setup ## CPU profiling (30s)
	@$(BENCH_SCRIPTS)/cpu-profile.sh

bench-profile: bench-setup ## Full profiling session (~100s)
	@$(BENCH_SCRIPTS)/full-profile.sh

bench-watch: ## Real-time memory monitoring (Ctrl+C to stop)
	@$(BENCH_SCRIPTS)/watch-memory.sh

bench-results: ## Show latest benchmark results
	@echo "=== Latest Benchmark Results ==="
	@if [ -d $(BENCH_RESULTS) ] && [ -n "$$(ls -A $(BENCH_RESULTS) 2>/dev/null)" ]; then \
		echo ""; \
		echo "Startup:"; \
		ls -t $(BENCH_RESULTS)/startup_*.txt 2>/dev/null | head -1 | xargs tail -n +2 || echo "  No results"; \
		echo ""; \
		echo "Playback:"; \
		ls -t $(BENCH_RESULTS)/playback_*_summary.txt 2>/dev/null | head -1 | xargs tail -n +2 || echo "  No results"; \
		echo ""; \
		echo "Memory Profile:"; \
		ls -t $(BENCH_RESULTS)/memory_profile_*_summary.txt 2>/dev/null | head -1 | xargs tail -n +2 || echo "  No results"; \
		echo ""; \
		echo "All results in: $(BENCH_RESULTS)/"; \
	else \
		echo "No results found. Run 'make bench-all' first."; \
	fi

bench-analyze: ## Analyze results and identify memory hogs
	@$(BENCH_SCRIPTS)/analyze-results.sh

bench-clean: ## Clean benchmark results
	@echo "Cleaning benchmark results..."
	@rm -rf $(BENCH_RESULTS)/*
	@echo "✓ Clean complete"

# ── Homebrew release workflow ─────────────────────────────────────────────────
# Usage:
#   make release          — tag current Cargo.toml version, push, publish release, update formula
#   make release-patch    — bump patch version (0.1.0 → 0.1.1), then release
#   make release-minor    — bump minor version (0.1.x → 0.2.0), then release

release-patch: ## Bump patch version and release
	cargo install cargo-edit 2>/dev/null || true
	cargo set-version --bump patch
	$(MAKE) release

release-minor: ## Bump minor version and release
	cargo install cargo-edit 2>/dev/null || true
	cargo set-version --bump minor
	$(MAKE) release

release: ## Tag, push, wait for CI binaries, then update Homebrew tap
	@echo "Releasing v$(VERSION)..."
	git add Cargo.toml Cargo.lock
	git diff --cached --quiet || git commit -m "Bump version to v$(VERSION)"
	git tag v$(VERSION)
	git push origin main
	git push origin v$(VERSION)
	@echo "Tag pushed. CI will build binaries and create the GitHub release."
	$(MAKE) bump-formula

wait-for-release-assets: ## Wait until both arch tarballs are attached to the GH release
	@echo "Waiting for v$(VERSION) release assets (arm64 + x86_64)..."
	@for i in $$(seq 1 90); do \
		ASSETS=$$(gh release view v$(VERSION) --repo program247365/looper --json assets --jq '.assets[].name' 2>/dev/null || true); \
		ARM=$$(echo "$$ASSETS" | grep -c "looper-aarch64-apple-darwin.tar.gz" || true); \
		X86=$$(echo "$$ASSETS" | grep -c "looper-x86_64-apple-darwin.tar.gz" || true); \
		if [ "$$ARM" = "1" ] && [ "$$X86" = "1" ]; then \
			echo "Both assets present."; \
			exit 0; \
		fi; \
		printf "  (%02d/90) waiting... arm64=%s x86_64=%s\n" "$$i" "$$ARM" "$$X86"; \
		sleep 10; \
	done; \
	echo "Timed out waiting for release assets. Check: gh run list --repo program247365/looper"; \
	exit 1

bump-formula: wait-for-release-assets ## Update tap formula with prebuilt binary URLs + SHA256s
	@set -e; \
	  ARM_URL="https://github.com/program247365/looper/releases/download/v$(VERSION)/looper-aarch64-apple-darwin.tar.gz"; \
	  X86_URL="https://github.com/program247365/looper/releases/download/v$(VERSION)/looper-x86_64-apple-darwin.tar.gz"; \
	  echo "Computing SHA256s..."; \
	  ARM_SHA=$$(curl -fsSL "$$ARM_URL" | shasum -a 256 | awk '{print $$1}'); \
	  X86_SHA=$$(curl -fsSL "$$X86_URL" | shasum -a 256 | awk '{print $$1}'); \
	  echo "  arm64:  $$ARM_SHA"; \
	  echo "  x86_64: $$X86_SHA"; \
	  rm -rf $(TAP_DIR); \
	  git clone $(TAP_REPO) $(TAP_DIR); \
	  bash scripts/render-formula.sh "$(VERSION)" "$$ARM_SHA" "$$X86_SHA" > $(TAP_DIR)/Formula/looper.rb; \
	  cd $(TAP_DIR) && git add Formula/looper.rb && \
	    git commit -m "Update looper to v$(VERSION)" && \
	    git push origin main; \
	  rm -rf $(TAP_DIR); \
	  echo "Done. Users now get a prebuilt binary on 'brew upgrade looper'."

smoke-test: ## Verify the published formula installs the prebuilt binary cleanly
	@echo "Smoke-testing prebuilt install for v$(VERSION)..."
	@brew tap program247365/tap >/dev/null 2>&1 || true
	@echo "==> Refreshing tap..."
	@brew update --quiet
	@echo "==> Asserting formula uses prebuilt-binary install path..."
	@brew cat program247365/tap/looper | grep -q 'bin.install "looper"' || \
	  (echo "FAIL: formula is not on the prebuilt path"; exit 1)
	@echo "==> Asserting tap formula version matches Cargo.toml..."
	@TAP_VERSION=$$(brew cat program247365/tap/looper | awk -F'"' '/^  version /{print $$2; exit}'); \
	  if [ "$$TAP_VERSION" != "$(VERSION)" ]; then \
	    echo "FAIL: tap has v$$TAP_VERSION but Cargo.toml is v$(VERSION) — run 'make bump-formula' first"; \
	    exit 1; \
	  fi; \
	  echo "  tap version: $$TAP_VERSION"
	@echo "==> Reinstalling..."
	@if brew list --versions program247365/tap/looper >/dev/null 2>&1; then \
	  brew reinstall program247365/tap/looper; \
	else \
	  brew install program247365/tap/looper; \
	fi
	@echo "==> Verifying binary..."
	@INSTALLED=$$(brew list --versions program247365/tap/looper | awk '{print $$2}'); \
	  if [ "$$INSTALLED" != "$(VERSION)" ]; then \
	    echo "FAIL: brew installed v$$INSTALLED but expected v$(VERSION)"; \
	    exit 1; \
	  fi; \
	  echo "  installed: v$$INSTALLED"
	@LOOPER_BIN="$$(brew --prefix)/bin/looper"; \
	  "$$LOOPER_BIN" --help >/dev/null 2>&1 && echo "  --help:   OK" || \
	    (echo "FAIL: $$LOOPER_BIN --help failed"; exit 1)
	@echo ""
	@echo "Smoke test passed for v$(VERSION)."
