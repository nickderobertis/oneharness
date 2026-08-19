"""Exercise the built wheel through its installed public import and real CLI."""

from __future__ import annotations

import email
import os
import subprocess
import sys
import tempfile
import zipfile
from contextlib import ExitStack
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]

# Must begin with `oneharness_core::io::scratch::PREFIX`, the prefix
# `scripts/check-temp-leaks.sh` sweeps for; `scripts/check-scratch-prefixes.sh`
# holds the two in step. Spelled here rather than imported from `test.scratch`
# because this file runs as a script, which has no package to import from.
PREFIX = "oneharness-python-"


def scratch_dir(stack: ExitStack, tag: str) -> Path:
    """Return a scratch directory `stack` removes, including when main() raises.

    There is no test framework here to hang a teardown on, so the caller's
    :class:`contextlib.ExitStack` is the hook — it unwinds on the way out of a
    failed run exactly as it does on a clean one.
    """
    return Path(stack.enter_context(tempfile.TemporaryDirectory(prefix=f"{PREFIX}{tag}-")))


def cargo_version() -> str:
    """Read the workspace's single release version without a TOML dependency."""
    in_package = False
    for line in (ROOT / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("["):
            in_package = line == "[package]"
        elif in_package and line.startswith("version = "):
            return line.split('"')[1]
    raise AssertionError("Cargo.toml has no root package version")


def main(stack: ExitStack) -> None:
    """Build, inspect, install offline, and consume the release-stamped wheels."""
    version = cargo_version()
    staged = subprocess.run(
        ["node", "scripts/python-sdk-pack.mjs"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    wheel_dir = scratch_dir(stack, "wheelhouse")
    subprocess.run(
        ["uv", "build", "--wheel", "--out-dir", str(wheel_dir), staged],
        cwd=ROOT,
        check=True,
    )
    subprocess.run(
        ["uv", "build", "--wheel", "--out-dir", str(wheel_dir), str(ROOT)],
        cwd=ROOT,
        check=True,
    )
    sdk_wheels = list(wheel_dir.glob("oneharness_sdk-*.whl"))
    cli_wheels = list(wheel_dir.glob("oneharness_cli-*.whl"))
    if len(sdk_wheels) != 1 or len(cli_wheels) != 1:
        raise AssertionError(
            f"expected one SDK and one CLI wheel, found SDK={sdk_wheels}, CLI={cli_wheels}"
        )

    with zipfile.ZipFile(sdk_wheels[0]) as archive:
        metadata_name = next(
            name for name in archive.namelist() if name.endswith(".dist-info/METADATA")
        )
        metadata = email.message_from_bytes(archive.read(metadata_name))
        if not any(
            name.endswith("oneharness_sdk/_generated/schemas.json") for name in archive.namelist()
        ):
            raise AssertionError("Python SDK wheel omitted its generated runtime schemas")
    assert metadata["Name"] == "oneharness-sdk"
    assert metadata["Version"] == version
    assert metadata["Requires-Python"] == ">=3.9"
    assert f"oneharness-cli=={version}" in metadata.get_all("Requires-Dist", [])

    environment = scratch_dir(stack, "installed") / "venv"
    subprocess.run(
        [
            "uv",
            "venv",
            "--offline",
            "--python",
            sys.executable,
            str(environment),
        ],
        cwd=ROOT,
        check=True,
    )
    scripts = environment / ("Scripts" if os.name == "nt" else "bin")
    python = scripts / ("python.exe" if os.name == "nt" else "python")
    subprocess.run(
        [
            "uv",
            "pip",
            "install",
            "--offline",
            "--python",
            str(python),
            str(cli_wheels[0]),
            str(sdk_wheels[0]),
        ],
        cwd=ROOT,
        check=True,
    )

    consumer = """
import asyncio
import sys
from importlib.metadata import requires, version
from oneharness_sdk import OneHarness, __version__

async def main():
    if __version__ != sys.argv[1]:
        raise AssertionError(f"SDK version {__version__} != {sys.argv[1]}")
    if version("oneharness-cli") != sys.argv[1]:
        raise AssertionError("installed CLI version does not match the SDK")
    if f"oneharness-cli=={sys.argv[1]}" not in (requires("oneharness-sdk") or []):
        raise AssertionError("installed SDK metadata lost its exact CLI requirement")
    harnesses = await OneHarness(env={"ONEHARNESS_NO_CONFIG": "1"}).list()
    if not any(item["id"] == "codex" for item in harnesses):
        raise AssertionError("installed SDK did not return the installed CLI registry")

asyncio.run(main())
"""
    env = {
        **os.environ,
        "PATH": os.pathsep.join((str(scripts), os.environ.get("PATH", ""))),
    }
    subprocess.run(
        [str(python), "-c", consumer, version],
        cwd=environment,
        env=env,
        check=True,
    )


if __name__ == "__main__":
    with ExitStack() as scratch_stack:
        main(scratch_stack)
