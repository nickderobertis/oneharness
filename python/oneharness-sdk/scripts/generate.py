"""Generate Python SDK schemas and public wire types from Rust metadata."""

from __future__ import annotations

import argparse
import copy
import difflib
import json
import re
import subprocess
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[3]
OUTPUT = ROOT / "python" / "oneharness-sdk" / "src" / "oneharness_sdk" / "_generated"
INPUT_ROOTS = (
    "run_options",
    "history_lookup",
    "history_list_options",
    "history_watch_options",
)


def snake_case(value: str) -> str:
    """Convert the Rust bundle's Node-facing camel case to Python snake case."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", value).lower()


def pythonize(node: Any) -> Any:
    """Rename object property declarations without touching map keys."""
    if isinstance(node, list):
        return [pythonize(item) for item in node]
    if not isinstance(node, dict):
        return node
    result: dict[str, Any] = {}
    for key, value in node.items():
        if key == "properties":
            result[key] = {snake_case(name): pythonize(child) for name, child in value.items()}
        elif key == "required":
            result[key] = [snake_case(name) for name in value]
        else:
            result[key] = pythonize(value)
    return result


def property_map(node: Any, result: dict[str, str]) -> None:
    """Collect the inverse top-level spelling map used when building CLI argv."""
    if isinstance(node, list):
        for item in node:
            property_map(item, result)
        return
    if not isinstance(node, dict):
        return
    for name in node.get("properties", {}):
        python = snake_case(name)
        previous = result.setdefault(python, name)
        if previous != name:
            raise RuntimeError(f"input properties {previous!r} and {name!r} collide as {python!r}")
    for value in node.values():
        property_map(value, result)


def type_expression(schema: dict[str, Any]) -> str:
    """Render the useful subset needed by strict SDK input TypedDicts."""
    if "$ref" in schema:
        name = schema["$ref"].rsplit("/", 1)[-1]
        return "dict[str, str]" if name == "HistoryLabels" else "str"
    kind = schema.get("type")
    if kind == "string":
        return "str"
    if kind == "boolean":
        return "bool"
    if kind == "integer":
        return "int"
    if kind == "number":
        return "float"
    if kind == "array":
        return f"Sequence[{type_expression(schema['items'])}]"
    if kind == "object":
        additional = schema.get("additionalProperties")
        if isinstance(additional, dict):
            return f"dict[str, {type_expression(additional)}]"
        return "dict[str, Any]"
    if "oneOf" in schema and all(item.get("const") is not None for item in schema["oneOf"]):
        return "str"
    return "Any"


def typed_dict(name: str, schema: dict[str, Any]) -> list[str]:
    """Generate one TypedDict while keeping required fields statically required."""
    properties = schema.get("properties", {})
    required = set(schema.get("required", []))
    optional_name = f"_{name}Optional"
    lines = [f"class {optional_name}(TypedDict, total=False):"]
    optional = [(key, value) for key, value in properties.items() if key not in required]
    if optional:
        lines.extend(f"    {key}: {type_expression(value)}" for key, value in optional)
    else:
        lines.append("    pass")
    lines.extend(("", "", f"class {name}({optional_name}):"))
    required_fields = [(key, value) for key, value in properties.items() if key in required]
    if required_fields:
        lines.extend(f"    {key}: {type_expression(value)}" for key, value in required_fields)
    else:
        lines.append("    pass")
    return lines


def types_module(bundle: dict[str, Any]) -> str:
    """Generate public Python input shapes and validated output aliases."""
    run = pythonize(bundle["run_options"])
    history_list = pythonize(bundle["history_list_options"])
    watch = pythonize(bundle["history_watch_options"])
    lines = [
        '"""Generated from oneharness-core. Do not edit."""',
        "",
        "from __future__ import annotations",
        "",
        "from collections.abc import Sequence",
        "from typing import Any, TypedDict",
        "",
        "",
        *typed_dict("RunOptions", run),
        "",
        "",
        *typed_dict("HistoryListOptions", history_list),
        "",
        "",
        *typed_dict("HistoryWatchOptions", watch),
        "",
        "",
        "HistoryLookup = dict[str, Any]",
        "RunReport = dict[str, Any]",
        "RunStreamEnvelope = dict[str, Any]",
        "HistoryRecord = dict[str, Any]",
        "HistoryStreamEnvelope = dict[str, Any]",
        "HarnessInfo = dict[str, Any]",
        "Detection = dict[str, Any]",
        "",
    ]
    return "\n".join(lines)


def generated_files() -> dict[str, bytes]:
    """Run the Rust generator and return deterministic package files."""
    output = subprocess.check_output(
        [
            "cargo",
            "run",
            "-q",
            "-p",
            "oneharness",
            "--features",
            "sdk-schema",
            "--example",
            "generate_sdk_schema",
        ],
        cwd=ROOT,
        encoding="utf-8",
        text=True,
    )
    bundle = json.loads(output)
    schemas = copy.deepcopy(bundle)
    input_keys: dict[str, dict[str, str]] = {}
    for root in INPUT_ROOTS:
        keys: dict[str, str] = {}
        property_map(bundle[root], keys)
        input_keys[root] = dict(sorted(keys.items()))
        schemas[root] = pythonize(bundle[root])
    # Rust's untagged enum schema is `anyOf`, matching the generated Zod union:
    # the deliberately overlapping `{session, last: true}` selector is valid and
    # the clients give `last` priority when constructing argv.
    if "oneOf" in schemas["history_lookup"]:  # pragma: no cover - drift guard
        raise RuntimeError("HistoryLookup must remain a non-exclusive anyOf union")
    return {
        "__init__.py": b'"""Rust-generated Python SDK assets."""\n',
        "schemas.json": (json.dumps(schemas, indent=2, sort_keys=True) + "\n").encode(),
        "input-keys.json": (json.dumps(input_keys, indent=2, sort_keys=True) + "\n").encode(),
        "../_generated_types.py": types_module(bundle).encode(),
    }


def drift(path: Path, expected: bytes) -> str:
    """Render why one generated file differs, so --check names the exact change."""
    if not path.exists():
        return f"{path}: missing"
    actual = path.read_bytes()
    try:
        lines = difflib.unified_diff(
            actual.decode().splitlines(),
            expected.decode().splitlines(),
            fromfile=f"{path} (checked in)",
            tofile=f"{path} (generated)",
            lineterm="",
            n=2,
        )
        return "\n".join(lines)
    except UnicodeDecodeError:  # pragma: no cover - generated assets are text
        return f"{path}: {len(actual)} bytes on disk, {len(expected)} generated"


def main() -> int:
    """Write generated files, or report drift under --check."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    stale: list[str] = []
    for relative, content in generated_files().items():
        path = OUTPUT / relative
        if args.check:
            if not path.exists() or path.read_bytes() != content:
                stale.append(drift(path, content))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    if stale:
        print("\n".join(stale))
        print("generated Python SDK contracts are stale; run just python-sdk-generate")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
