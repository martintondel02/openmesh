#!/usr/bin/env python3
"""Verify the help corpus is internally consistent.

The same content/help files are embedded in the bridge (build.rs), rendered in the web Help
Center and returned by MCP `openmesh_help`, so metadata and files must never drift apart.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
HELP = ROOT / "content" / "help"
REQUIRED_FIELDS = {"slug", "title", "summary", "audience", "minutes", "related"}
AUDIENCES = {"beginner", "advanced", "both"}
MIN_BODY_LINES = 20  # skeletons are placeholders; real topics are 80-250 lines


def main() -> int:
    problems: list[str] = []
    index_path = HELP / "index.json"
    if not index_path.is_file():
        print(f"missing {index_path.relative_to(ROOT)}", file=sys.stderr)
        return 1
    try:
        index = json.loads(index_path.read_text())
    except json.JSONDecodeError as exc:
        print(f"index.json is not valid JSON: {exc}", file=sys.stderr)
        return 1

    topics = index.get("topics")
    if not isinstance(topics, list) or not topics:
        print("index.json has no topics", file=sys.stderr)
        return 1

    seen: set[str] = set()
    for entry in topics:
        missing = REQUIRED_FIELDS - entry.keys()
        if missing:
            problems.append(f"{entry.get('slug', '?')}: missing fields {sorted(missing)}")
            continue
        slug = entry["slug"]
        if slug in seen:
            problems.append(f"duplicate slug {slug}")
        seen.add(slug)
        if entry["audience"] not in AUDIENCES:
            problems.append(f"{slug}: invalid audience {entry['audience']!r}")
        if not isinstance(entry["related"], list):
            problems.append(f"{slug}: related must be a list")
        path = HELP / f"{slug}.md"
        if not path.is_file():
            problems.append(f"{slug}: missing {path.relative_to(ROOT)}")
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        if not lines or not lines[0].startswith("# "):
            problems.append(f"{slug}: first line must be an H1 title")
        elif lines[0][2:].strip() != entry["title"]:
            problems.append(f"{slug}: H1 {lines[0][2:].strip()!r} != index title {entry['title']!r}")
        if len(lines) < MIN_BODY_LINES:
            problems.append(f"{slug}: body only {len(lines)} lines (< {MIN_BODY_LINES}); expand the topic")
        for rel in entry["related"]:
            if rel not in {t["slug"] for t in topics if t.get("slug")}:
                problems.append(f"{slug}: related topic {rel!r} is not in the index")

    on_disk = {p.stem for p in HELP.glob("*.md")}
    for extra in on_disk - seen:
        problems.append(f"{extra}.md exists but is not in index.json")

    if problems:
        print("\n".join(problems), file=sys.stderr)
        return 1
    print(f"help corpus OK ({len(topics)} topics)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
