"""Exercise the built wheel through its installed public import and real CLI."""

from __future__ import annotations

import email
import os
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SUFFIX = ".exe" if os.name == "nt" else ""
BINARY = ROOT / "target" / "debug" / f"oneharness{SUFFIX}"


def cargo_version() -> str:
    """Read the workspace's single release version without a TOML dependency."""
    in_package = False
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("["):
            in_package = line == "[package]"
        elif in_package and line.startswith("version = "):
            return line.split('"')[1]
    raise AssertionError("Cargo.toml has no root package version")


def main() -> None:
    """Build, inspect, extract, and consume the release-stamped wheel."""
    version = cargo_version()
    staged = subprocess.run(
        ["node", "scripts/python-sdk-pack.mjs"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    wheel_dir = ROOT / "python" / "dist" / "wheels"
    subprocess.run(
        ["uv", "build", "--wheel", "--out-dir", str(wheel_dir), staged],
        cwd=ROOT,
        check=True,
    )
    wheels = list(wheel_dir.glob("oneharness_sdk-*.whl"))
    if len(wheels) != 1:
        raise AssertionError(f"expected exactly one Python SDK wheel, found {wheels}")

    extracted = Path(tempfile.mkdtemp(prefix="oneharness-python-wheel-"))
    with zipfile.ZipFile(wheels[0]) as archive:
        metadata_name = next(
            name for name in archive.namelist() if name.endswith(".dist-info/METADATA")
        )
        metadata = email.message_from_bytes(archive.read(metadata_name))
        archive.extractall(extracted)
    assert metadata["Name"] == "oneharness-sdk"
    assert metadata["Version"] == version
    assert metadata["Requires-Python"] == ">=3.9"
    assert f"oneharness-cli=={version}" in metadata.get_all("Requires-Dist", [])

    consumer = """
import asyncio
import sys
from oneharness_sdk import OneHarness, __version__

async def main():
    if __version__ != sys.argv[2]:
        raise AssertionError(f"SDK version {__version__} != {sys.argv[2]}")
    harnesses = await OneHarness(executable=sys.argv[1], env={"ONEHARNESS_NO_CONFIG": "1"}).list()
    if not any(item["id"] == "codex" for item in harnesses):
        raise AssertionError("wheel SDK did not return the packaged CLI registry")

asyncio.run(main())
"""
    env = {**os.environ, "PYTHONPATH": str(extracted)}
    subprocess.run(
        [sys.executable, "-c", consumer, str(BINARY), version],
        cwd=extracted,
        env=env,
        check=True,
    )


if __name__ == "__main__":
    main()
