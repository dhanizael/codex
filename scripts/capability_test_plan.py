#!/usr/bin/env python3
"""Plan sandbox-safe and host-required Nextest invocations."""

from __future__ import annotations

import argparse
import json
import re
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import tomllib


@dataclass(frozen=True)
class CapabilityRegistry:
    host_filter: str
    signatures: tuple[str, ...]


def load_registry(path: Path) -> CapabilityRegistry:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if data.get("schema_version") != 1:
        raise ValueError("unsupported test capability schema_version")

    entries = data.get("host_required")
    if not isinstance(entries, list) or not entries:
        raise ValueError("host_required must be a non-empty array")
    seen_ids: set[str] = set()
    filters = []
    for entry in entries:
        entry_id = required_string(entry, "id")
        if entry_id in seen_ids:
            raise ValueError(f"duplicate host_required id: {entry_id}")
        seen_ids.add(entry_id)
        required_string(entry, "reason")
        required_strings(entry, "capabilities")
        filters.append(normalize_filter(required_string(entry, "filter")))

    signatures = data.get("environment_failure_signatures")
    if (
        not isinstance(signatures, list)
        or not signatures
        or not all(isinstance(signature, str) and signature for signature in signatures)
    ):
        raise ValueError(
            "environment_failure_signatures must be a non-empty string array"
        )
    return CapabilityRegistry(
        host_filter=" | ".join(
            f"({filter_expression})" for filter_expression in filters
        ),
        signatures=tuple(signatures),
    )


def required_string(raw: dict[str, Any], field: str) -> str:
    value = raw.get(field)
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{field} must be a non-empty string")
    return value.strip()


def required_strings(raw: dict[str, Any], field: str) -> tuple[str, ...]:
    value = raw.get(field)
    if (
        not isinstance(value, list)
        or not value
        or not all(isinstance(item, str) and item for item in value)
    ):
        raise ValueError(f"{field} must be a non-empty string array")
    return tuple(value)


def normalize_filter(expression: str) -> str:
    return " ".join(expression.split())


def planned_commands(
    registry: CapabilityRegistry, test_args: list[str]
) -> dict[str, Any]:
    base = ["just", "test", *test_args]
    safe = [*base, "-E", f"not ({registry.host_filter})"]
    host = [*base, "-E", registry.host_filter]
    return {
        "sandboxSafe": safe,
        "hostRequired": host,
        "hostPrefixRule": ["just", "test"],
    }


FAIL_LINE = re.compile(r"^\s*(?:TRY \d+ )?FAIL\s+\[[^]]+\]\s+\([^)]*\)\s+(.+?)\s*$")
STATUS_LINE = re.compile(r"^\s*(?:(?:TRY \d+ )?FAIL|PASS)\s+\[[^]]+\]")


def classify_failures(registry: CapabilityRegistry, log: str) -> dict[str, Any]:
    lines = log.splitlines()
    environment_ids: list[str] = []
    assertion_ids: list[str] = []
    classified_ids: set[str] = set()
    for index, line in enumerate(lines):
        match = FAIL_LINE.match(line)
        if not match:
            continue
        test_id = match.group(1)
        if test_id in classified_ids:
            continue
        classified_ids.add(test_id)
        block_end = next(
            (
                candidate
                for candidate in range(index + 1, len(lines))
                if STATUS_LINE.match(lines[candidate])
            ),
            len(lines),
        )
        block = "\n".join(lines[index:block_end])
        target = (
            environment_ids
            if any(signature in block for signature in registry.signatures)
            else assertion_ids
        )
        if test_id not in target:
            target.append(test_id)
    return {
        "environmentFailures": environment_ids,
        "assertionFailures": assertion_ids,
        "retryFilter": exact_test_filter(environment_ids),
    }


def exact_test_filter(test_ids: list[str]) -> str | None:
    if not test_ids:
        return None
    test_names = [test_id.split(maxsplit=1)[-1] for test_id in test_ids]
    return " | ".join(f"test(={test_name})" for test_name in test_names)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--registry",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "test-capabilities.toml",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    plan = subparsers.add_parser("plan")
    plan.add_argument("--json", action="store_true")
    plan.add_argument("test_args", nargs=argparse.REMAINDER)
    classify = subparsers.add_parser("classify")
    classify.add_argument("log", type=Path)
    classify.add_argument("--json", action="store_true")
    subparsers.add_parser("check")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        registry = load_registry(args.registry)
        if args.command == "check":
            print("Test capability registry: OK")
            return 0
        if args.command == "plan":
            test_args = args.test_args
            if test_args[:1] == ["--"]:
                test_args = test_args[1:]
            plan = planned_commands(registry, test_args)
            if args.json:
                print(json.dumps(plan, indent=2))
            else:
                print(f"sandbox-safe: {shlex.join(plan['sandboxSafe'])}")
                print(f"host-required: {shlex.join(plan['hostRequired'])}")
            return 0

        report = classify_failures(registry, args.log.read_text(errors="replace"))
        if args.json:
            print(json.dumps(report, indent=2))
        else:
            print(f"environment failures: {len(report['environmentFailures'])}")
            print(f"assertion failures: {len(report['assertionFailures'])}")
            if report["retryFilter"]:
                print(f"exact retry filter: {report['retryFilter']}")
        return 1 if report["assertionFailures"] else 0
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        print(f"capability test plan error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
