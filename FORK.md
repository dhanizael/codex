# Codex Enhanced: Engineering Case Study

Codex Enhanced is an independently maintained patch layer over OpenAI Codex. It focuses on runtime integrity, bounded resource handling, deterministic diagnostics, and maintainable long-running agent workflows.

This document explains the engineering work that differs from upstream. For official Codex documentation, use the [OpenAI Codex repository](https://github.com/openai/codex) and [official documentation](https://developers.openai.com/codex).

## Project identity

- Maintainer: [Hamdani (`dhanizael`)](https://github.com/dhanizael)
- Upstream: [`openai/codex`](https://github.com/openai/codex)
- Patch base: [`rust-v0.146.0`](https://github.com/openai/codex/releases/tag/rust-v0.146.0)
- Implementation language: primarily Rust, with Python, TypeScript, Bazel, and Starlark tooling inherited from upstream
- License: Apache-2.0, preserving the upstream license and attribution
- Status: experimental community fork, not affiliated with or endorsed by OpenAI

## Problem statement

Coding agents spend much of their time at operating-system and orchestration boundaries. Those boundaries are failure-prone:

- command output may exceed the model context or grow without bound on disk;
- timed-out hooks may leave descendants running;
- configuration changes may invalidate assumptions in an active session;
- local capability restrictions can make tests fail for environmental rather than behavioral reasons;
- expensive turns may begin even when deterministic local blockers already exist;
- a locally built binary can drift from the source tree that a maintainer believes it represents.

Codex Enhanced treats these as explicit engineering problems rather than incidental edge cases.

## Patch architecture

```text
OpenAI Codex rust-v0.146.0
             |
             v
   enhancement ledger
      |      |      |
      v      v      v
   runtime  workflow  maintainer tooling
      |         |            |
      +---------+------------+
                |
                v
       codex-enhanced binary
```

The fork is not a detached rewrite. Each custom capability is registered in [`enhancements.toml`](enhancements.toml) with:

- a stable enhancement identifier;
- the implementation commit;
- affected architectural areas;
- focused verification commands.

[`scripts/enhancement_ledger.py`](scripts/enhancement_ledger.py) validates that the declared commits exist in the patch layer, detects unmanaged commits, and can compare patch IDs and changed files against a newer upstream reference.

## Enhancement catalog

### Runtime hardening

The initial patch adds configuration and session guards, safer execution-output processing, model metadata checks, and explicit protocol errors. The goal is to turn silent drift into diagnosable state while keeping failure behavior bounded.

### Selective local compaction retry

Local compaction is retried only for failures classified as safe to retry. This avoids a generic retry loop that could repeat deterministic failures or hide a deeper error.

### Model-instruction guards

Instruction-affecting configuration participates in fingerprints and session locks. A running session can therefore detect incompatible changes instead of unknowingly continuing with altered behavioral assumptions.

### Runtime provenance

The CLI reports build provenance and source parity through its doctor surface. This makes it easier to answer a deceptively important debugging question: "Does this binary represent the source tree I am inspecting?"

### Session pinning

Important sessions can be pinned in the resume picker. The work includes app-server integration, TUI behavior, and snapshot coverage for compact, wide, and pinned states.

### Complete, file-backed unified-exec output

Model-visible output remains bounded while the complete stream can be retained in a private file. The tool response references that file only when truncation occurs, preserving debuggability without injecting unbounded content into model context.

### Capability-aware tests

Focused tests are partitioned by the capabilities they require. This distinguishes a product regression from a test that cannot run inside a restricted sandbox and keeps local verification useful.

### Bounded output retention

File-backed output has an explicit cleanup policy. This complements spill-to-disk behavior by preventing historical output from becoming an unbounded storage system.

### Deterministic local preflight

`codex doctor --preflight` checks deterministic local blockers before an expensive agent turn. Human-readable and redacted JSON reporting support both interactive diagnosis and automation.

### Hook command hardening

Hook output is streamed through bounded handling, timeouts terminate process trees, and platform-specific containment is respected. This reduces the chance that an extension hook can hang a session or leave orphaned work behind.

## Validation strategy

The project follows the upstream repository's focused-test-first guidance:

1. Format changed Rust code with `just fmt`.
2. Run tests for the affected crate through `just test -p <crate>`.
3. Run scoped fixes with `just fix -p <crate>` for substantial Rust changes.
4. Update and review `insta` snapshots for user-visible TUI changes.
5. Run broader suites only when shared crates or cross-cutting behavior justify the cost.

The exact prescribed commands for every patch are part of the machine-readable enhancement ledger rather than duplicated in prose.

## Local development

```shell
git clone https://github.com/dhanizael/codex.git
cd codex
git switch enhanced
cd codex-rs

# Fast feedback
cargo check -p codex-cli

# Focused test example
just test -p codex-cli

# Final local binary
cargo build --release --locked -p codex-cli
```

The repository pins Rust 1.95 in `codex-rs/rust-toolchain.toml`. The release profile is intentionally expensive; normal iteration should use checks, focused tests, and development builds.

## Maintaining parity with upstream

The two remotes should remain explicit:

```shell
git remote -v
# origin    git@github.com:dhanizael/codex.git
# upstream  https://github.com/openai/codex.git
```

A maintenance cycle is:

1. Fetch `upstream` without modifying the active patch branch.
2. Run the enhancement ledger against `upstream/main` to identify overlap and conflict risk.
3. Rebase or replay one coherent enhancement at a time onto a reviewed upstream base.
4. Re-run each enhancement's focused verification.
5. Update the pinned `upstream_base` only after the complete patch layer is validated.

This model favors explainable patches over a permanently diverging monolithic branch.

## Scope and non-goals

This fork is a research and engineering portfolio project. It does not claim to be an official Codex distribution, does not replace OpenAI support channels, and does not promise compatibility with every upstream release.

The project will not intentionally weaken Codex sandbox boundaries, bypass approval controls, or conceal behavior from users. Reliability improvements should remain auditable and should preserve safe defaults.

## Attribution

Codex Enhanced is based on OpenAI Codex and retains the upstream Apache-2.0 license. "OpenAI" and "Codex" are names associated with OpenAI. This community fork is independently maintained and is not affiliated with, maintained by, sponsored by, or endorsed by OpenAI.
