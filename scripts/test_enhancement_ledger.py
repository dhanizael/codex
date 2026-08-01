#!/usr/bin/env python3

import importlib.util
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("enhancement_ledger.py")
SPEC = importlib.util.spec_from_file_location("enhancement_ledger", SCRIPT)
assert SPEC and SPEC.loader
ledger = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = ledger
SPEC.loader.exec_module(ledger)


class EnhancementLedgerTests(unittest.TestCase):
    def test_reports_coverage_and_upstream_overlap(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "--quiet", "--initial-branch=main")
            git(root, "config", "user.name", "Codex Test")
            git(root, "config", "user.email", "codex@example.invalid")
            (root / "shared.txt").write_text("base\n")
            git(root, "add", "shared.txt")
            git(root, "commit", "--quiet", "-m", "base")
            base = git(root, "rev-parse", "HEAD")
            git(root, "tag", "upstream-base")

            (root / "shared.txt").write_text("enhanced\n")
            git(root, "commit", "--quiet", "-am", "enhancement")
            enhancement = git(root, "rev-parse", "HEAD")

            git(root, "checkout", "--quiet", "-b", "upstream", base)
            (root / "shared.txt").write_text("upstream\n")
            git(root, "commit", "--quiet", "-am", "upstream change")
            git(root, "checkout", "--quiet", "main")

            manifest = root / "enhancements.toml"
            manifest.write_text(
                textwrap.dedent(
                    f'''\
                    schema_version = 1
                    upstream_base = "upstream-base"

                    [[enhancements]]
                    id = "sample"
                    title = "Sample enhancement"
                    kind = "enhancement"
                    commits = ["{enhancement}"]
                    areas = ["sample"]
                    tests = ["sample test"]
                    '''
                )
            )

            report = ledger.build_report(root, manifest, "upstream")

            self.assertTrue(report["valid"])
            self.assertEqual(report["unmanagedCommits"], [])
            self.assertEqual(report["enhancements"][0]["status"], "conflict-risk")
            self.assertEqual(
                report["enhancements"][0]["upstreamOverlap"], ["shared.txt"]
            )

    def test_unmanaged_commit_fails_check_state(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "--quiet", "--initial-branch=main")
            git(root, "config", "user.name", "Codex Test")
            git(root, "config", "user.email", "codex@example.invalid")
            (root / "file.txt").write_text("base\n")
            git(root, "add", "file.txt")
            git(root, "commit", "--quiet", "-m", "base")
            git(root, "tag", "upstream-base")
            (root / "file.txt").write_text("patch\n")
            git(root, "commit", "--quiet", "-am", "unmanaged")
            manifest = root / "enhancements.toml"
            manifest.write_text(
                textwrap.dedent(
                    """\
                    schema_version = 1
                    upstream_base = "upstream-base"
                    """
                )
            )

            report = ledger.build_report(root, manifest, None)

            self.assertFalse(report["valid"])
            self.assertEqual(len(report["unmanagedCommits"]), 1)

    def test_ledger_only_commit_is_classified_as_management(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            git(root, "init", "--quiet", "--initial-branch=main")
            git(root, "config", "user.name", "Codex Test")
            git(root, "config", "user.email", "codex@example.invalid")
            (root / "base.txt").write_text("base\n")
            git(root, "add", "base.txt")
            git(root, "commit", "--quiet", "-m", "base")
            git(root, "tag", "upstream-base")
            manifest = root / "enhancements.toml"
            manifest.write_text('schema_version = 1\nupstream_base = "upstream-base"\n')
            git(root, "add", "enhancements.toml")
            git(root, "commit", "--quiet", "-m", "add ledger")

            report = ledger.build_report(root, manifest, None)

            self.assertTrue(report["valid"])
            self.assertEqual(len(report["ledgerManagementCommits"]), 1)
            self.assertEqual(report["unmanagedCommits"], [])


def git(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    ).stdout.strip()


if __name__ == "__main__":
    unittest.main()
