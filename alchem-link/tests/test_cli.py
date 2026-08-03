import os
import subprocess
import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
ENV = {**dict(os.environ), "PYTHONPATH": str(REPO_ROOT / "src")}


def _run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, "-m", "alchem_link.cli", *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        env=ENV,
    )


class CLITests(unittest.TestCase):
    def test_blueprint_command(self):
        result = _run("blueprint")
        self.assertEqual(result.returncode, 0)
        self.assertIn('"project"', result.stdout)
        self.assertIn("Alchemy x Chainlink Developer Package", result.stdout)

    def test_list_command(self):
        result = _run("list")
        self.assertEqual(result.returncode, 0)
        self.assertIn("blueprint", result.stdout)
        self.assertIn("integration", result.stdout)

    def test_recipes_command_all(self):
        result = _run("recipes")
        self.assertEqual(result.returncode, 0)
        self.assertIn("oracle-backed-automation", result.stdout)
        self.assertIn("ccip-cross-chain-transfer", result.stdout)

    def test_recipes_command_by_id(self):
        result = _run("recipes", "real-time-data-pipeline")
        self.assertEqual(result.returncode, 0)
        self.assertIn("real-time-data-pipeline", result.stdout)

    def test_recipes_unknown_id_exits(self):
        result = _run("recipes", "does-not-exist")
        self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
