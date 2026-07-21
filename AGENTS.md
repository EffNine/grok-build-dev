# AGENTS.md

## Cursor Cloud specific instructions

This repo is the Rust source for **Grok Build (`grok`)** — a terminal-based AI coding agent (single-product Cargo workspace). It is a client CLI/TUI; there are no local databases, containers, or `docker compose` to run. Standard build/test/lint/run commands live in `README.md` (Building from source / Development) — use those.

### Build/run caveats (non-obvious)
- The build needs **`protoc`**. It is provided hermetically by the DotSlash wrapper at `bin/protoc`, which requires the `dotslash` binary to be installed and on `PATH` (`cargo install dotslash`). The proto build script (`crates/build/xai-proto-build/src/find_protoc.rs`) auto-discovers `bin/protoc` by walking up parent directories, so you do **not** need to add `bin/` to `PATH` or set `$PROTOC` when building from within the workspace — just have `dotslash` installed.
- The Rust toolchain is pinned by `rust-toolchain.toml` (1.92.0) and is auto-installed by `rustup` on the first cargo invocation.
- Root `Cargo.toml` is generated — treat it as read-only; edit per-crate manifests.
- Full-workspace builds are slow (a clean debug build of `xai-grok-pager-bin` takes ~5 min on 4 cores). Prefer targeting a specific crate: `cargo check -p <crate>`, `cargo test -p <crate>`, `cargo clippy -p <crate>`.
- The built binary artifact is `target/debug/xai-grok-pager` (release: `target/release/xai-grok-pager`); official installs ship it as `grok`.

### Running the agent (authentication required)
- The core product (the agent responding to a prompt / editing files) requires xAI auth against remote endpoints (`api.x.ai`). Provide either an `XAI_API_KEY` env var, or complete an interactive login (`grok login`, or the on-launch OAuth device-code flow shown in the TUI).
- Without credentials, the TUI still launches and renders the OAuth login screen, and headless mode (`grok -p "..."`) exits with a clear "Not signed in" error. All unit/integration tests mock these endpoints, so `cargo test`/`cargo build` need no live services or credentials.
