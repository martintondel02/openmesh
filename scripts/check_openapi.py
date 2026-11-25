#!/usr/bin/env python3
"""Verify docs/api/openapi.json: valid OpenAPI 3.1 and every bridge REST route documented.

The bridge is the source of truth for its routes: this check scans the HTTP module for quoted
`"/api/..."` literals (including path prefixes such as `"/api/nodes/"`) and fails if any of them
is not covered by a documented path in the spec. It also performs the structural checks CI needs
(OpenAPI version, info, paths, operations, local `$ref` resolution, bearer auth, agent header).

Stdlib only, so it runs in the docs CI job without extra dependencies.

Usage:
    python3 scripts/check_openapi.py [--spec docs/api/openapi.json] [--src crates/openmesh-bridge/src/http]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_SPEC = ROOT / "docs" / "api" / "openapi.json"
DEFAULT_SRC = ROOT / "crates" / "openmesh-bridge" / "src" / "http"

METHODS = {"get", "post", "put", "patch", "delete", "head", "options", "trace"}
# Literals that are generic dispatcher prefixes, not routes of their own.
GENERIC = {"/api", "/api/"}
PATH_LITERAL = re.compile(r'"(/api/[^"\\]*)"')


def strip_comments(text: str) -> str:
    """Remove // comments while keeping string literals (URLs contain //)."""
    out: list[str] = []
    i = 0
    in_string = False
    while i < len(text):
        ch = text[i]
        if in_string:
            out.append(ch)
            if ch == "\\" and i + 1 < len(text):
                out.append(text[i + 1])
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue
        if ch == '"':
            in_string = True
            out.append(ch)
            i += 1
            continue
        if ch == "/" and i + 1 < len(text) and text[i + 1] == "/":
            while i < len(text) and text[i] != "\n":
                i += 1
            continue
        out.append(ch)
        i += 1
    return "".join(out)


def collect_routes(src: Path) -> list[str]:
    routes: set[str] = set()
    files = sorted(p for p in src.rglob("*.rs") if p.is_file())
    if not files:
        raise SystemExit(f"no Rust sources found under {src}")
    for path in files:
        text = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
        for match in PATH_LITERAL.finditer(text):
            literal = match.group(1)
            if literal in GENERIC:
                continue
            if literal.startswith("/api/") and len(literal) > len("/api/"):
                routes.add(literal)
    return sorted(routes)


def route_covered(route: str, spec_paths: list[str]) -> bool:
    """Return True when a source literal is represented by a documented path."""
    if route.endswith("/"):
        # Prefix literal such as "/api/nodes/": needs a templated path below it.
        prefix = route
        return any(p.startswith(prefix) and "{" in p for p in spec_paths)
    if route in spec_paths:
        return True
    # A literal may be a prefix without its trailing slash in code; allow templates.
    prefix = route if route.endswith("/") else route + "/"
    return any(p.startswith(prefix) and "{" in p for p in spec_paths)


def resolve_pointer(doc: Any, ref: str) -> Any:
    if not ref.startswith("#/"):
        return None
    node: Any = doc
    for raw in ref[2:].split("/"):
        key = raw.replace("~1", "/").replace("~0", "~")
        if not isinstance(node, dict) or key not in node:
            return None
        node = node[key]
    return node


def find_refs(value: Any, out: list[str]) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            if key == "$ref" and isinstance(child, str):
                out.append(child)
            else:
                find_refs(child, out)
    elif isinstance(value, list):
        for child in value:
            find_refs(child, out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--spec", type=Path, default=DEFAULT_SPEC)
    parser.add_argument("--src", type=Path, default=DEFAULT_SRC)
    args = parser.parse_args()

    problems: list[str] = []
    spec_path: Path = args.spec
    src: Path = args.src
    if not spec_path.is_file():
        print(f"missing spec: {spec_path}", file=sys.stderr)
        return 1
    try:
        spec = json.loads(spec_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"{spec_path} is not valid JSON: {exc}", file=sys.stderr)
        return 1

    version = spec.get("openapi", "")
    if not isinstance(version, str) or not version.startswith("3.1."):
        problems.append(f"openapi must be 3.1.x, got {version!r}")
    info = spec.get("info")
    if not isinstance(info, dict) or not info.get("title") or not info.get("version"):
        problems.append("info.title and info.version are required")
    paths = spec.get("paths")
    if not isinstance(paths, dict) or not paths:
        problems.append("paths must be a non-empty object")
        paths = {}
    path_keys = list(paths)
    for path in path_keys:
        if not path.startswith("/"):
            problems.append(f"path {path!r} must start with /")
        item = paths[path]
        if not isinstance(item, dict):
            problems.append(f"{path}: path item must be an object")
            continue
        operations = [m for m in item if m.lower() in METHODS]
        if not operations:
            problems.append(f"{path}: no operations")
        for method in operations:
            op = item[method]
            if not isinstance(op, dict):
                problems.append(f"{method.upper()} {path}: operation must be an object")
                continue
            if not op.get("summary"):
                problems.append(f"{method.upper()} {path}: missing summary")
            responses = op.get("responses")
            if not isinstance(responses, dict) or not responses:
                problems.append(f"{method.upper()} {path}: missing responses")

    schemes = spec.get("components", {}).get("securitySchemes", {})
    if "bearerAuth" not in schemes:
        problems.append("components.securitySchemes.bearerAuth is required")
    elif schemes["bearerAuth"].get("scheme") != "bearer":
        problems.append("bearerAuth must use the http bearer scheme")
    headers = spec.get("components", {}).get("headers", {})
    if "X-OpenMesh-Agent" not in headers:
        problems.append("components.headers.X-OpenMesh-Agent is required")

    refs: list[str] = []
    find_refs(spec, refs)
    for ref in sorted(set(refs)):
        if resolve_pointer(spec, ref) is None:
            problems.append(f"unresolved $ref {ref}")

    routes = collect_routes(src)
    missing = [route for route in routes if not route_covered(route, path_keys)]

    print(f"spec: {spec_path.relative_to(ROOT) if spec_path.is_relative_to(ROOT) else spec_path}")
    print(f"paths: {len(path_keys)}, operations: {sum(len([m for m in item if m.lower() in METHODS]) for item in paths.values())}")
    print(f"bridge route literals: {len(routes)} in {src.relative_to(ROOT) if src.is_relative_to(ROOT) else src}")
    if missing:
        print("undocumented bridge routes:", file=sys.stderr)
        for route in missing:
            print(f"  - {route}", file=sys.stderr)
        problems.append(f"{len(missing)} bridge route literal(s) missing from the spec")
    if problems:
        print("\nPROBLEMS:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1
    print("openapi OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
