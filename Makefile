.PHONY: help build build-release build-macos run test test-all install clean \
        release release-patch release-minor bump-formula

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

release: ## Tag, push, create GitHub release, and update Homebrew formula
	@echo "Releasing v$(VERSION)..."
	git add Cargo.toml Cargo.lock
	git diff --cached --quiet || git commit -m "Bump version to v$(VERSION)"
	git tag v$(VERSION)
	git push origin main
	git push origin v$(VERSION)
	gh release create v$(VERSION) \
		--repo program247365/looper \
		--title "v$(VERSION)" \
		--generate-notes
	$(MAKE) bump-formula

bump-formula: ## Update Homebrew tap formula to current version
	$(eval SHA256 := $(shell curl -sL "https://github.com/program247365/looper/archive/refs/tags/v$(VERSION).tar.gz" | shasum -a 256 | awk '{print $$1}'))
	@echo "SHA256: $(SHA256)"
	rm -rf $(TAP_DIR)
	git clone $(TAP_REPO) $(TAP_DIR)
	sed -i '' \
		-e 's|url ".*"|url "https://github.com/program247365/looper/archive/refs/tags/v$(VERSION).tar.gz"|' \
		-e 's|sha256 ".*"|sha256 "$(SHA256)"|' \
		$(TAP_DIR)/Formula/looper.rb
	cd $(TAP_DIR) && git add Formula/looper.rb && \
		git commit -m "Update looper to v$(VERSION)" && \
		git push origin main
	rm -rf $(TAP_DIR)
	@echo "Done. Install with: brew tap program247365/tap && brew install looper"
