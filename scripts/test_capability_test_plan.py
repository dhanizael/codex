#!/usr/bin/env python3

import importlib.util
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("capability_test_plan.py")
SPEC = importlib.util.spec_from_file_location("capability_test_plan", SCRIPT)
assert SPEC and SPEC.loader
planner = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = planner
SPEC.loader.exec_module(planner)


class CapabilityTestPlanTests(unittest.TestCase):
    def test_plan_partitions_one_invocation_without_repeating_filters(self) -> None:
        registry = planner.CapabilityRegistry(
            host_filter="test(host_only)",
            signatures=("Operation not permitted",),
        )

        plan = planner.planned_commands(registry, ["-p", "codex-core", "unified_exec"])

        self.assertEqual(
            plan["sandboxSafe"],
            [
                "just",
                "test",
                "-p",
                "codex-core",
                "unified_exec",
                "-E",
                "not (test(host_only))",
            ],
        )
        self.assertEqual(plan["hostRequired"][-2:], ["-E", "test(host_only)"])

    def test_classifier_retries_only_environment_failures_by_exact_id(self) -> None:
        registry = planner.CapabilityRegistry(
            host_filter="test(host_only)",
            signatures=("Operation not permitted",),
        )
        log = textwrap.dedent(
            """\
            TRY 1 FAIL [ 0.01s] (1/2) crate::suite::needs_port
              Error: Operation not permitted
            TRY 1 FAIL [ 0.01s] (2/2) crate::suite::bad_assertion
              assertion failed: left == right
            TRY 2 FAIL [ 0.01s] (1/2) crate::suite::needs_port
            """
        )

        report = planner.classify_failures(registry, log)

        self.assertEqual(report["environmentFailures"], ["crate::suite::needs_port"])
        self.assertEqual(report["assertionFailures"], ["crate::suite::bad_assertion"])
        self.assertEqual(report["retryFilter"], "test(=crate::suite::needs_port)")

    def test_exact_retry_filter_removes_nextest_binary_prefix(self) -> None:
        self.assertEqual(
            planner.exact_test_filter(
                ["codex-core::all suite::exec_policy::needs_loopback"]
            ),
            "test(=suite::exec_policy::needs_loopback)",
        )

    def test_registry_rejects_duplicate_ids(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "capabilities.toml"
            path.write_text(
                textwrap.dedent(
                    """\
                    schema_version = 1
                    environment_failure_signatures = ["denied"]

                    [[host_required]]
                    id = "duplicate"
                    reason = "first"
                    capabilities = ["loopback-bind"]
                    filter = "test(first)"

                    [[host_required]]
                    id = "duplicate"
                    reason = "second"
                    capabilities = ["path-alias"]
                    filter = "test(second)"
                    """
                )
            )

            with self.assertRaisesRegex(ValueError, "duplicate host_required id"):
                planner.load_registry(path)


if __name__ == "__main__":
    unittest.main()
