[← Back to README](../README.md)

# Development

```shell
make run           # play fixture file (tests/fixtures/sound.mp3)
make test          # run tests
make build         # debug build
make build-release # optimized release binary
```

Useful direct commands:

```shell
cargo build
cargo build --release
cargo test
```

> **Note:** `vergen` is pinned to `9.0.6` in `Cargo.lock` to keep
> librespot-core's build script compiling. If a build fails with a vergen-lib
> trait mismatch after `cargo update`, re-pin with
> `cargo update -p vergen --precise 9.0.6`.

## Releasing

```shell
make release-patch    # bump patch version and release
make release-minor    # bump minor version and release
make smoke-test       # (optional) verify the published formula installs cleanly
```

`make release-patch` / `make release-minor` runs end-to-end:

1. Bumps the version in `Cargo.toml` and commits it
2. Tags `v<version>` and pushes the tag
3. The `Release` GitHub Actions workflow (`.github/workflows/release.yml`) fires
   on the tag, builds an `aarch64-apple-darwin` binary on a `macos-14` runner,
   and attaches it to the GitHub release
4. `make bump-formula` (auto-invoked) polls the release, computes the SHA256,
   regenerates the Homebrew formula via `scripts/render-formula.sh`, and pushes
   the update to [`program247365/homebrew-tap`](https://github.com/program247365/homebrew-tap)

Total wall-clock time is typically 3–4 minutes (most of it the arm64 cargo build
on CI).

`make smoke-test` then reinstalls the formula on your machine and asserts:

- the formula uses the prebuilt-binary install path (`bin.install "looper"`)
- the tap version matches `Cargo.toml`
- `looper --help` runs successfully

If you need to recover from a partial release (e.g. CI flaked between tag push
and formula update), re-run `make bump-formula` directly — it is idempotent and
will wait for the asset, then push to the tap.
