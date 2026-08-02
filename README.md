<div align="center">

# Codex Enhanced

**A maintainer-focused Codex CLI fork for safer execution, stronger diagnostics, and more reliable long-running agent workflows.**

[![Enhanced patch layer](https://github.com/dhanizael/codex/actions/workflows/enhanced-ci.yml/badge.svg?branch=enhanced)](https://github.com/dhanizael/codex/actions/workflows/enhanced-ci.yml)
[![Upstream base](https://img.shields.io/badge/upstream-rust--v0.146.0-5c6ac4)](https://github.com/openai/codex/releases/tag/rust-v0.146.0)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

[Enhancements](#enhancements) | [Engineering case study](FORK.md) | [Build](#build-the-fork) | [Upstream Codex](#upstream-codex-cli)

</div>

> [!IMPORTANT]
> This is an independent community fork maintained by [Hamdani](https://github.com/dhanizael). It is based on [OpenAI Codex](https://github.com/openai/codex), but it is not affiliated with, maintained by, or endorsed by OpenAI.

## Why this fork exists

Agent reliability is often determined at the unglamorous edges: subprocess cleanup, bounded output, deterministic diagnostics, configuration integrity, and test behavior under restricted environments. Codex Enhanced explores those edges as a transparent patch layer over a pinned upstream release.

The fork carries an explicit patch ledger spanning the Rust CLI, core runtime, hooks, TUI, configuration, protocol, and maintainer tooling. Every enhancement maps to its implementation commit, affected area, and prescribed verification in [`enhancements.toml`](enhancements.toml).

## Enhancements

| Area | What changed | Engineering goal |
| --- | --- | --- |
| Runtime hardening | Configuration guards, safer output processing, and runtime checks | Fail safely and make invalid state visible |
| Unified exec | File-backed complete output with bounded retention | Preserve evidence without unbounded context or disk growth |
| Hook execution | Bounded streams, timeouts, and process-tree termination | Prevent runaway hooks and orphaned processes |
| Local doctor | Deterministic preflight checks and redacted JSON reports | Detect blockers before an expensive agent turn |
| Session workflow | Pinning in the resume picker with snapshot coverage | Keep important work easy to find |
| Compaction | Selective retry for local compaction failures | Recover only when retry is safe and meaningful |
| Test infrastructure | Capability-aware test partitioning | Separate sandbox-safe tests from capability-sensitive tests |
| Provenance | Build provenance and source-parity reporting | Make the running binary auditable |

See [FORK.md](FORK.md) for the design rationale, patch architecture, validation strategy, and maintenance model.

## Try it locally

Prerequisites: Rust 1.95, Git, `just`, and the native build dependencies required by upstream Codex.

```shell
git clone https://github.com/dhanizael/codex.git
cd codex
git switch enhanced
cd codex-rs
cargo build --release --locked -p codex-cli
install -Dm755 target/release/codex "$HOME/.local/bin/codex-enhanced"
codex-enhanced --version
codex-enhanced doctor --preflight
```

For fast development, prefer `cargo check -p <crate>` and focused `just test -p <crate>` runs. Reserve release builds for final validation because upstream's optimized profile includes expensive link-time optimization and debug information.

## Patch-layer audit

```shell
python3 scripts/enhancement_ledger.py check
python3 scripts/enhancement_ledger.py report --upstream-ref upstream/main
python3 -m unittest scripts/test_enhancement_ledger.py
```

The ledger reports missing or unmanaged patch commits and highlights overlap with newer upstream changes.

## Upstream Codex CLI

The documentation below is retained from the upstream project. Its installers and package-manager commands install the official OpenAI distribution, not Codex Enhanced.

<p align="center"><strong>Codex CLI</strong> is a coding agent from OpenAI that runs locally on your computer.
<p align="center">
  <img src="https://github.com/openai/codex/blob/main/.github/codex-cli-splash.png" alt="Codex CLI splash" width="80%" />
</p>
</br>
If you want Codex in your code editor (VS Code, Cursor, Windsurf), <a href="https://developers.openai.com/codex/ide">install in your IDE.</a>
</br>If you want the desktop app experience, run <code>codex app</code> or visit <a href="https://chatgpt.com/codex?app-landing-page=true">the Codex App page</a>.
</br>If you are looking for the <em>cloud-based agent</em> from OpenAI, <strong>Codex Web</strong>, go to <a href="https://chatgpt.com/codex">chatgpt.com/codex</a>.</p>

---

## Quickstart

### Installing and running Codex CLI

Run the following on Mac or Linux to install Codex CLI:

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | sh
```

Run the following on Windows to install Codex CLI:

```shell
powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 | iex"
```

The standalone installers download from `https://releases.openai.com/codex` by default and fall back to GitHub Releases if a metadata or asset download is unavailable. To force GitHub Releases, set `CODEX_INSTALLER_USE_RELEASES_OPENAI_COM` to `false` (`0` and `no` are also accepted):

```shell
curl -fsSL https://chatgpt.com/codex/install.sh | CODEX_INSTALLER_USE_RELEASES_OPENAI_COM=false sh
```

```powershell
$env:CODEX_INSTALLER_USE_RELEASES_OPENAI_COM='false'; irm https://chatgpt.com/codex/install.ps1 | iex
```

Codex CLI can also be installed via the following package managers:

```shell
# Install using npm
npm install -g @openai/codex
```

```shell
# Install using Homebrew
brew install --cask codex
```

Then simply run `codex` to get started.

<details>
<summary>You can also go to the <a href="https://github.com/openai/codex/releases/latest">latest GitHub Release</a> and download the appropriate binary for your platform.</summary>

Each GitHub Release contains many executables, but in practice, you likely want one of these:

- macOS
  - Apple Silicon/arm64: `codex-aarch64-apple-darwin.tar.gz`
  - x86_64 (older Mac hardware): `codex-x86_64-apple-darwin.tar.gz`
- Linux
  - x86_64: `codex-x86_64-unknown-linux-musl.tar.gz`
  - arm64: `codex-aarch64-unknown-linux-musl.tar.gz`

Each archive contains a single entry with the platform baked into the name (e.g., `codex-x86_64-unknown-linux-musl`), so you likely want to rename it to `codex` after extracting it.

</details>

### Using Codex with your ChatGPT plan

Run `codex` and select **Sign in with ChatGPT**. We recommend signing into your ChatGPT account to use Codex as part of your Plus, Pro, Business, Edu, or Enterprise plan. [Learn more about what's included in your ChatGPT plan](https://help.openai.com/en/articles/11369540-codex-in-chatgpt).

You can also use Codex with an API key, but this requires [additional setup](https://developers.openai.com/codex/auth#sign-in-with-an-api-key).

## Docs

- [**Codex Documentation**](https://developers.openai.com/codex)
- [**Contributing**](./docs/contributing.md)
- [**Installing & building**](./docs/install.md)
- [**Open source fund**](./docs/open-source-fund.md)

This repository is licensed under the [Apache-2.0 License](LICENSE).
