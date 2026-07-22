# Grok Build (`grok`) — Free/BYOK Fork

A terminal-based AI coding agent. Bring your own API key and model endpoint — no subscription required.

This fork removes all SpaceXAI/xAI service integration (OAuth login, telemetry, subscription checks, managed configs, and SuperGrok upsells). The only supported auth method is a provider API key. Grok works with any OpenAI-compatible endpoint.

---

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/effnine/grok-build-dev/main/install.sh | bash
```

Pin a version:

```sh
curl -fsSL https://raw.githubusercontent.com/effnine/grok-build-dev/main/install.sh | bash -s 0.2.107
```

This downloads the matching GitHub Release binary into `~/.grok/bin` and puts it on your `PATH`. macOS (arm64/x86_64) and Linux (x86_64/arm64) are supported.

> Requires a published [GitHub Release](https://github.com/effnine/grok-build-dev/releases) with `grok-{version}-{os}-{arch}` assets. Tag `vX.Y.Z` (or run the **Release** workflow) to publish one. Latest with binaries: **v0.2.107**.

## Quick Start (BYOK)

Export your provider key and base URL, then launch the TUI:

```sh
export XAI_API_KEY="sk-..."
export GROK_MODELS_BASE_URL="https://api.openai.com/v1"
grok
```

Or launch the TUI first and configure it inline with the `/byok` slash command:

```text
/byok sk-... https://api.openai.com/v1
```

Grok will fetch the model catalog from `<base_url>/models` and list all models available under your key.

### Per-model configuration

Add a `[model.<id>]` block to `~/.grok/config.toml` for models that need their own endpoint or key:

```toml
[model.claude-sonnet-4]
model = "claude-sonnet-4-20250514"
base_url = "https://api.anthropic.com/v1"
api_key = "sk-ant-..."
name = "Claude Sonnet 4"
description = "Anthropic Claude Sonnet 4"
context_window = 200000
```

### Legacy environment variable

The old `GROK_CODE_XAI_API_KEY` name is still accepted as a fallback.

---

## Building from source

Requirements:

- **Rust** — the toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml); `rustup` installs it automatically on first build.
- **[DotSlash](https://dotslash-cli.com)** — required so hermetic tools under [`bin/`](bin/) (notably [`bin/protoc`](bin/protoc)) can download and run. Install it and ensure `dotslash` is on your `PATH` before building:

  ```sh
  cargo install dotslash
  # or: prebuilt packages — https://dotslash-cli.com/docs/installation/
  /usr/bin/env dotslash --help   # sanity check
  ```

- **protoc** — proto codegen resolves [`bin/protoc`](bin/protoc) via DotSlash, or falls back to a `protoc` on `PATH` / `$PROTOC`.
- macOS and Linux are supported build hosts; Windows builds are best-effort and not currently tested from this tree.

```sh
cargo run -p xai-grok-pager-bin              # build + launch the TUI
cargo build -p xai-grok-pager-bin --release  # release binary: target/release/xai-grok-pager
cargo check -p xai-grok-pager-bin            # fast validation
```

The binary artifact is named `xai-grok-pager`; rename or symlink it to `grok` if you want to call it that way.

---

## Repository layout

| Path | Contents |
|------|----------|
| `crates/codegen/xai-grok-pager-bin` | Composition-root package; builds the `xai-grok-pager` binary |
| `crates/codegen/xai-grok-pager` | The TUI: scrollback, prompt, modals, rendering |
| `crates/codegen/xai-grok-shell` | Agent runtime + leader/stdio/headless entry points |
| `crates/codegen/xai-grok-tools` | Tool implementations (terminal, file edit, search, ...) |
| `crates/codegen/xai-grok-workspace` | Host filesystem, VCS, execution, checkpoints |
| `crates/codegen/...` | The rest of the CLI crate closure (config, MCP, markdown, sandbox, ...) |
| `crates/common/`, `crates/build/`, `prod/mc/` | Small shared leaf crates pulled in by the closure |
| `third_party/` | Vendored upstream source (Mermaid diagram stack) — see below |

> [!IMPORTANT]
> The root `Cargo.toml` (workspace members, dependency versions, lints, profiles) is **generated** — treat it as read-only. Prefer editing per-crate `Cargo.toml` files.

---

## Development

```sh
cargo check -p <crate>        # always target specific crates; full-workspace builds are slow
cargo test -p xai-grok-config # per-crate tests
cargo clippy -p <crate>       # lint config: clippy.toml at the repo root
cargo fmt --all               # rustfmt.toml at the repo root
```

---

## License

First-party code in this repository is licensed under the **Apache License, Version 2.0** — see [`LICENSE`](LICENSE).

Third-party and vendored code remains under its original licenses. See:

- [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) — crates.io / git dependencies, bundled UI themes, and in-tree source ports (including openai/codex and sst/opencode tool implementations)
- [`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md) — crate-local notice for the codex and opencode ports (license texts + Apache §4(b) change notice)
- [`third_party/NOTICE`](third_party/NOTICE) — vendored Mermaid-stack index
