.PHONY: help build build-release build-macos run test test-all install clean

BINARY := looper
INSTALL_DIR := /usr/local/bin
FIXTURE := tests/fixtures/sound.mp3

help: ## Show available targets
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make \033[36m<target>\033[0m\n\nTargets:\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

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
