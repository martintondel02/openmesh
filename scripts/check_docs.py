#!/usr/bin/env python3
"""Verify required documents exist and that relative markdown links resolve."""
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REQUIRED = [
    "README.md",
    "LICENSE",
    "NOTICE",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "CODE_OF_CONDUCT.md",
    "CHANGELOG.md",
    "docs/ROADMAP.md",
    "docs/architecture/ARCHITECTURE.md",
    "docs/architecture/PROTOCOL.md",
    "docs/architecture/WEBSOCKET-API.md",
    "docs/architecture/COMPANION-API.md",
]
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")

problems: list[str] = []

for rel in REQUIRED:
    if not (ROOT / rel).is_file():
        problems.append(f"missing required document: {rel}")

for md in ROOT.rglob("*.md"):
    if any(part in {".git", "node_modules", "target", ".pio", "build"} for part in md.parts):
        continue
    text = md.read_text(encoding="utf-8", errors="ignore")
    for target in LINK_RE.findall(text):
        if target.startswith(("http://", "https://", "mailto:", "#")):
            continue
        target = target.split("#", 1)[0]
        if not target:
            continue
        resolved = (md.parent / target).resolve()
        if not resolved.exists():
            problems.append(f"{md.relative_to(ROOT)}: broken link -> {target}")

if problems:
    print("\n".join(problems), file=sys.stderr)
    sys.exit(1)
print(f"docs OK ({len(REQUIRED)} required files present)")
