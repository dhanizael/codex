# Codex Enhanced Feature Inventory

This inventory records the behavior carried by the enhanced patch layer over OpenAI Codex `rust-v0.146.0`. It is intentionally more granular than [`enhancements.toml`](enhancements.toml): one coherent implementation commit may contain several related capabilities, while the ledger tracks that commit as one auditable patch.

The inventory describes code that exists in the `showcase` branch. It does not imply affiliation with OpenAI, compatibility with every upstream release, or a measured reduction in API billing.

## At a glance

| Domain | Capability | Key behavior |
| --- | --- | --- |
| Context | Adaptive tool-output budgets | 6K tokens for normal output and 8K for failed commands by default |
| Context | Custom-tool history cap | Caps custom-tool output at 6K tokens even under a larger history policy |
| Context | Signal-preserving normalization | Coalesces repeated diagnostics, strips ANSI color, and extracts key failures |
| Evidence | Full unified-exec output | Stores the complete byte stream in a private file when model output is truncated |
| Storage | Bounded spill retention | Applies a 7-day retention window and 512 MiB global budget |
| Hooks | Bounded hook streams | Keeps a 1 MiB head/tail preview and spills larger output privately |
| Hooks | Process containment | Terminates timed-out hook process trees and preserves partial output |
| Diagnostics | Local preflight | Checks filesystem access, Codex home writes, loopback, shell, and PATH |
| Diagnostics | Build provenance | Reports commit, dirty state, source fingerprint, and enhancement revision |
| Diagnostics | Source parity | Detects whether the running binary matches the inspected source tree |
| Instructions | Exact-model tuning | Supports exact-slug augmentation and explicit full replacement files |
| Instructions | Integrity guards | Supports versioned SHA-256 checks, strict validation, and native fallback |
| Sessions | Instruction lock hygiene | Includes instruction tuning in config fingerprints and session locks |
| Sessions | Resume-picker pinning | Pins important sessions, sorts them first, and preserves selection |
| Reliability | Selective compaction retry | Retries transient failures only and honors server-provided delays |
| Reliability | Actionable conflict errors | Classifies HTTP retryability and explains recovery for stale-turn conflicts |
| Persistence | Transcript failure visibility | Warns when completed or interrupted turns fail to persist |
| Maintenance | Auditable patch layer | Maps capabilities to commits, areas, tests, and upstream-overlap risk |

## 1. Context-efficient tool output

### Adaptive budgets

Unified command output uses different default model-visible budgets:

- successful or still-running output: 6,000 tokens;
- failed command output: 8,000 tokens;
- explicit per-call limits still take precedence and remain bounded by the active truncation policy.

The larger error budget protects diagnostics without giving every successful command the same context allowance.

### Custom-tool history cap

Custom-tool output recorded in conversation history is capped at 6,000 tokens, including when the general history policy permits more. This prevents one tool result from occupying an outsized portion of future turns.

### Output normalization

Before command output becomes model-visible, the enhanced path can:

- coalesce three or more identical consecutive warning or error lines;
- remove ANSI Select Graphic Rendition color sequences;
- preserve non-color terminal control sequences rather than deleting arbitrary bytes;
- extract unique `error:`, `fatal:`, panic, source-location, and failed-test diagnostics from long failed output;
- retain standard head-and-tail truncation and omission metadata.

The full output is not silently discarded. Context optimization and evidence retention are separate concerns.

## 2. Complete output with bounded private storage

### File-backed unified-exec output

The unified-exec watcher writes the complete stream as it arrives instead of reconstructing it from an already truncated preview. When truncation affects the model-visible result, the result can expose the private local path containing the full output.

On Unix-like systems:

- output directories are mode `0700`;
- output files are mode `0600`;
- existing call-output files are not silently replaced;
- active markers protect files that are still being written.

### Cleanup policy

Background cleanup bounds retained tool output with two defaults:

- maximum age: 7 days;
- maximum aggregate storage: 512 MiB.

Expired sessions are reclaimed first, followed by the oldest eligible sessions until the storage budget is satisfied. The current session and live active markers remain protected; stale markers associated with dead processes can be reclaimed.

## 3. Hardened hook execution

Hook stdout and stderr are captured incrementally with a 1 MiB inline limit. Larger streams retain a bounded head/tail preview and spill the complete output to a private file.

Timeout handling preserves partial output and terminates descendants rather than only killing the immediate shell process. The implementation includes Unix process-group handling and Windows job-object integration.

## 4. Doctor and deterministic preflight

`codex doctor --preflight` runs local checks that can identify blockers before an expensive agent turn:

- working-directory readability;
- `CODEX_HOME` write capability through an actual probe;
- loopback bind capability;
- configured shell presence and executability;
- non-empty `PATH` visibility;
- relevant config, Git, sandbox, and state checks already exposed by doctor.

Hard filesystem failures produce a failed preflight. Optional capability limitations produce warnings with remediation instead of being misclassified as product failures. Human, summary, and redacted JSON output remain available.

## 5. Build provenance and source parity

The enhanced binary identifies itself with an enhanced version suffix and embeds build metadata, including:

- source commit;
- dirty-source state;
- source fingerprint;
- source root when available;
- enhancement revision;
- build platform and installation context.

Doctor compares the embedded provenance with the current source tree. A mismatch produces a warning and a rebuild/remediation path; unavailable source is distinguished from mismatched source.

## 6. Exact-model instruction tuning

Instruction files are keyed by exact model slug. Two modes are intentionally distinct:

- augmentation appends local guidance while preserving native model instructions and personality variables;
- replacement is an explicit advanced escape hatch that replaces the matching model's native instructions while preserving required personality handling.

Safety and integrity behavior includes:

- exact-slug matching rather than fuzzy model-family matching;
- an emergency switch that disables local files and falls back to native instructions;
- optional versioned SHA-256 guards;
- fail-closed handling for missing files, changed guarded content, invalid slugs, conflicting augmentation/replacement entries, and orphaned guards;
- rejection of reserved personality placeholders;
- augmentation limit of 4,000 bytes or approximately 1,000 tokens;
- replacement limit of 40,000 bytes or approximately 10,000 tokens;
- doctor visibility into active files and integrity metadata.

Instruction settings participate in config fingerprints and session-lock state so a resumed session can detect behaviorally significant changes.

## 7. Retry, conflict, and compaction behavior

Unexpected HTTP status errors are no longer all treated as equally retryable:

- request timeout, rate limiting, and server errors remain retryable;
- permanent client failures stop without consuming the retry budget;
- conflict responses include recovery guidance for stale or mismatched turn state.

Local compaction retries only retryable Codex errors and uses a provider/server retry delay when one is available, otherwise falling back to local backoff.

## 8. Transcript persistence visibility

If rollout persistence fails after a completed or interrupted turn, the user receives a warning that distinguishes in-memory availability from durable transcript storage. The warning explains that the turn may be lost if Codex exits before persistence recovers.

This avoids both silent loss and the misleading implication that persistence has already succeeded.

## 9. Session pinning

The resume picker supports `Alt+P` to pin or unpin a session through app-server metadata. Pinned sessions:

- render with a visible indicator;
- sort before unpinned sessions while preserving relative group order;
- retain the current selection after filtering and asynchronous pin updates;
- display inline errors if an update fails;
- have compact, wide, dense, and pinned-state snapshot coverage.

## 10. Maintainer workflow and reproducibility

### Capability-aware tests

The test planner separates sandbox-safe focused tests from tests requiring capabilities such as network access. This keeps restricted local environments from turning every capability limitation into a false regression signal.

### Enhancement ledger

[`enhancements.toml`](enhancements.toml) and [`scripts/enhancement_ledger.py`](scripts/enhancement_ledger.py) provide:

- commit ownership for each patch;
- affected subsystem inventory;
- prescribed focused verification;
- unmanaged-commit detection;
- patch-ID comparison with newer upstream work;
- changed-file overlap as a revalidation/conflict signal.

### Lockfile parity

Cargo workspace version changes are synchronized with the lockfile and Bazel lock expectations, reducing drift between supported build paths.

## Verification map

The machine-readable source of truth for exact commands is [`enhancements.toml`](enhancements.toml). Representative coverage includes:

- context caps and signal-preserving output tests;
- exact-byte spill, permissions, active-marker, and cleanup-policy tests;
- hook overflow, timeout, partial-output, and descendant-termination tests;
- preflight success, warning, and blocking-failure tests;
- source parity and build provenance tests;
- instruction guard, slug, size, fallback, augmentation, and replacement tests;
- retryability, conflict recovery, and non-retryable compaction tests;
- resume-picker sorting, rendering, selection, and snapshots;
- ledger and capability-planner unit tests.

Run the patch-layer checks with:

```shell
python3 scripts/enhancement_ledger.py check
python3 -m unittest scripts/test_enhancement_ledger.py scripts/test_capability_test_plan.py
```
