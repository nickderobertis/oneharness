"""Drift tests for Rust-to-Python contract generation."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SDK = ROOT / "python" / "oneharness-sdk"


class GenerationTests(unittest.TestCase):
    """Pin deterministic generated assets and actionable drift failures."""

    def test_checked_in_contracts_match_rust(self) -> None:
        """Run the generator's public check mode."""
        subprocess.run(
            [sys.executable, str(SDK / "scripts" / "generate.py"), "--check"],
            cwd=ROOT,
            check=True,
        )

    def test_missing_generated_file_is_reported_as_stale(self) -> None:
        """A clean checkout missing one artifact must fail without recreating it."""
        checkout = Path(tempfile.mkdtemp(prefix="oneharness-python-generate-"))
        try:
            subprocess.run(
                ["git", "checkout-index", f"--prefix={checkout.as_posix()}/", "-a"],
                cwd=ROOT,
                check=True,
            )
            shutil.copytree(SDK, checkout / "python" / "oneharness-sdk", dirs_exist_ok=True)
            missing = (
                checkout
                / "python"
                / "oneharness-sdk"
                / "src"
                / "oneharness_sdk"
                / "_generated"
                / "schemas.json"
            )
            missing.unlink()
            completed = subprocess.run(
                [
                    sys.executable,
                    str(checkout / "python" / "oneharness-sdk" / "scripts" / "generate.py"),
                    "--check",
                ],
                cwd=checkout,
                check=False,
                capture_output=True,
                text=True,
                env={**dict(os.environ), "CARGO_TARGET_DIR": str(ROOT / "target")},
            )
            self.assertEqual(completed.returncode, 1)
            self.assertIn("generated Python SDK contracts are stale", completed.stdout)
            self.assertFalse(missing.exists())
        finally:
            shutil.rmtree(checkout)


if __name__ == "__main__":
    unittest.main()
