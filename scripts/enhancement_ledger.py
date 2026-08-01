#!/usr/bin/env python3
"""Report the local enhancement patch layer against an optional upstream ref."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import tomllib

LEDGER_PATHS = {
    "enhancements.toml",
    "scripts/enhancement_ledger.py",
    "scripts/test_enhancement_ledger.py",
}


@dataclass(frozen=True)
class Enhancement:
    id: str
    title: str
    kind: str
    commits: tuple[str, ...]
    areas: tuple[str, ...]
    tests: tuple[str, ...]


def git(root: Path, *args: str, input_text: str | None = None) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        check=False,
        input=input_text,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        message = result.stderr.strip() or result.stdout.strip()
        raise RuntimeError(f"git {' '.join(args)} failed: {message}")
    return result.stdout.strip()


def load_manifest(path: Path) -> tuple[str, list[Enhancement]]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if data.get("schema_version") != 1:
        raise ValueError("unsupported enhancement ledger schema_version")
    upstream_base = data.get("upstream_base")
    if not isinstance(upstream_base, str) or not upstream_base:
        raise ValueError("upstream_base must be a non-empty string")

    enhancements = []
    seen_ids: set[str] = set()
    claimed_commits: set[str] = set()
    for raw in data.get("enhancements", []):
        enhancement = Enhancement(
            id=required_string(raw, "id"),
            title=required_string(raw, "title"),
            kind=required_string(raw, "kind"),
            commits=required_strings(raw, "commits"),
            areas=required_strings(raw, "areas"),
            tests=required_strings(raw, "tests"),
        )
        if enhancement.id in seen_ids:
            raise ValueError(f"duplicate enhancement id: {enhancement.id}")
        duplicate_commits = claimed_commits.intersection(enhancement.commits)
        if duplicate_commits:
            raise ValueError(
                f"commits claimed more than once: {', '.join(sorted(duplicate_commits))}"
            )
        seen_ids.add(enhancement.id)
        claimed_commits.update(enhancement.commits)
        enhancements.append(enhancement)
    return upstream_base, enhancements


def required_string(raw: dict[str, Any], field: str) -> str:
    value = raw.get(field)
    if not isinstance(value, str) or not value:
        raise ValueError(f"{field} must be a non-empty string")
    return value


def required_strings(raw: dict[str, Any], field: str) -> tuple[str, ...]:
    value = raw.get(field)
    if (
        not isinstance(value, list)
        or not value
        or not all(isinstance(item, str) and item for item in value)
    ):
        raise ValueError(f"{field} must be a non-empty string array")
    return tuple(value)


def resolve_commit(root: Path, revision: str) -> str | None:
    try:
        return git(root, "rev-parse", "--verify", f"{revision}^{{commit}}")
    except RuntimeError:
        return None


def patch_id(root: Path, revision: str) -> str:
    patch = git(root, "show", "--pretty=format:", "--no-ext-diff", "--binary", revision)
    if not patch:
        return ""
    result = subprocess.run(
        ["git", "patch-id", "--stable"],
        check=False,
        input=patch,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(f"git patch-id failed: {result.stderr.strip()}")
    return result.stdout.split(maxsplit=1)[0] if result.stdout.strip() else ""


def changed_files(root: Path, revision: str) -> set[str]:
    output = git(root, "diff-tree", "--no-commit-id", "--name-only", "-r", revision)
    return set(output.splitlines()) if output else set()


def upstream_facts(
    root: Path, base: str, upstream_ref: str
) -> tuple[set[str], set[str]]:
    resolved = resolve_commit(root, upstream_ref)
    if resolved is None:
        raise ValueError(f"upstream ref does not resolve: {upstream_ref}")
    commits = git(root, "rev-list", f"{base}..{resolved}").splitlines()
    patch_ids = {value for commit in commits if (value := patch_id(root, commit))}
    files = git(root, "diff", "--name-only", f"{base}..{resolved}")
    return patch_ids, set(files.splitlines()) if files else set()


def build_report(
    root: Path,
    manifest: Path,
    upstream_ref: str | None,
) -> dict[str, Any]:
    upstream_base, enhancements = load_manifest(manifest)
    base_commit = resolve_commit(root, upstream_base)
    if base_commit is None:
        raise ValueError(f"upstream_base does not resolve: {upstream_base}")

    patch_commits = git(
        root, "rev-list", "--reverse", f"{base_commit}..HEAD"
    ).splitlines()
    upstream_patch_ids: set[str] = set()
    upstream_files: set[str] = set()
    if upstream_ref:
        upstream_patch_ids, upstream_files = upstream_facts(
            root, base_commit, upstream_ref
        )

    claimed: set[str] = set()
    rows = []
    invalid = False
    for enhancement in enhancements:
        resolved_commits = []
        missing_commits = []
        files: set[str] = set()
        exact_match = False
        for revision in enhancement.commits:
            commit = resolve_commit(root, revision)
            if commit is None or commit not in patch_commits:
                missing_commits.append(revision)
                continue
            claimed.add(commit)
            resolved_commits.append(commit)
            files.update(changed_files(root, commit))
            exact_match = exact_match or (
                bool(upstream_patch_ids)
                and patch_id(root, commit) in upstream_patch_ids
            )

        overlap = sorted(files.intersection(upstream_files))
        if missing_commits:
            status = "needs-revalidation"
            invalid = True
        elif exact_match:
            status = "exact-patch-match"
        elif upstream_ref and overlap:
            status = "conflict-risk"
        else:
            status = "active"
        rows.append(
            {
                "id": enhancement.id,
                "title": enhancement.title,
                "kind": enhancement.kind,
                "status": status,
                "commits": resolved_commits,
                "missingCommits": missing_commits,
                "areas": list(enhancement.areas),
                "tests": list(enhancement.tests),
                "changedFiles": sorted(files),
                "upstreamOverlap": overlap,
            }
        )

    unclaimed = [commit for commit in patch_commits if commit not in claimed]
    ledger_management = [
        commit
        for commit in unclaimed
        if changed_files(root, commit).issubset(LEDGER_PATHS)
    ]
    unmanaged = [commit for commit in unclaimed if commit not in ledger_management]
    if unmanaged:
        invalid = True
    return {
        "schemaVersion": 1,
        "repository": str(root),
        "head": git(root, "rev-parse", "HEAD"),
        "upstreamBase": upstream_base,
        "upstreamBaseCommit": base_commit,
        "comparedUpstreamRef": upstream_ref,
        "enhancements": rows,
        "ledgerManagementCommits": ledger_management,
        "unmanagedCommits": unmanaged,
        "valid": not invalid,
    }


def render_human(report: dict[str, Any]) -> str:
    lines = [
        f"Enhancement ledger: {report['repository']}",
        f"HEAD: {report['head']}",
        f"Upstream base: {report['upstreamBase']} ({report['upstreamBaseCommit'][:12]})",
    ]
    if report["comparedUpstreamRef"]:
        lines.append(f"Compared upstream: {report['comparedUpstreamRef']}")
    lines.append("")
    width = max((len(row["id"]) for row in report["enhancements"]), default=2)
    for row in report["enhancements"]:
        lines.append(f"{row['id']:<{width}}  {row['status']:<20}  {row['title']}")
        if row["upstreamOverlap"]:
            lines.append(f"{'':<{width}}  overlap: {len(row['upstreamOverlap'])} files")
    if report["unmanagedCommits"]:
        lines.append("")
        lines.append("Unmanaged patch commits:")
        lines.extend(f"  {commit}" for commit in report["unmanagedCommits"])
    lines.append("")
    lines.append(f"Ledger status: {'OK' if report['valid'] else 'FAIL'}")
    return "\n".join(lines)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command", choices=("report", "check"), nargs="?", default="report"
    )
    parser.add_argument(
        "--repo", type=Path, default=Path(__file__).resolve().parents[1]
    )
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--upstream-ref")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(git(args.repo.resolve(), "rev-parse", "--show-toplevel"))
    manifest = args.manifest or root / "enhancements.toml"
    try:
        report = build_report(root, manifest, args.upstream_ref)
    except (OSError, RuntimeError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"enhancement ledger error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, indent=2) if args.json else render_human(report))
    return 1 if args.command == "check" and not report["valid"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
