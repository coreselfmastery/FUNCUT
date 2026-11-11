#!/usr/bin/env python3
"""Deterministic unit tests for the local Geneva benchmark helpers."""

from __future__ import annotations

import importlib.util
import os
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = ROOT / "scripts" / "motion" / "benchmark-geneva.py"
SPEC = importlib.util.spec_from_file_location("benchmark_geneva", MODULE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load benchmark module: {MODULE_PATH}")
benchmark = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(benchmark)


class ParseJsonDocumentsTests(unittest.TestCase):
    def test_reads_concatenated_json_objects_with_whitespace(self) -> None:
        documents = benchmark.parse_json_documents(
            '{ "ok": true }\\n  {"scenes": 4}\\n'
        )
        self.assertEqual(documents, [{"ok": True}, {"scenes": 4}])

    def test_ignores_non_object_json_values(self) -> None:
        documents = benchmark.parse_json_documents(
            '[1, 2]\\n{"ok": true}\\nnull\\n"note"'
        )
        self.assertEqual(documents, [{"ok": True}])

    def test_rejects_truncated_or_non_json_output(self) -> None:
        for output in ('{"ok": true}\\n{', 'renderer failed'):
            with self.subTest(output=output):
                with self.assertRaises(ValueError):
                    benchmark.parse_json_documents(output)


class ProcessTreeRssTests(unittest.TestCase):
    def test_current_process_has_non_negative_rss_on_procfs(self) -> None:
        measured = benchmark.process_tree_rss_kib(os.getpid())
        if Path("/proc").is_dir():
            self.assertIsNotNone(measured)
            self.assertGreaterEqual(measured, 0)
        else:
            self.assertIsNone(measured)


if __name__ == "__main__":
    unittest.main(verbosity=2)
