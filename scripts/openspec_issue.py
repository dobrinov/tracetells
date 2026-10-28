#!/usr/bin/env python3
"""Converts between an OpenSpec change folder and the body of a GitHub issue.

    openspec_issue.py build <change-id> [--exploration FILE]   change folder -> issue body (stdout)
    openspec_issue.py extract <body-file> [--root DIR]         issue body -> openspec/changes/<id>/ (prints the id)

The issue layout is the one GitHub renders from the issue forms in
.github/ISSUE_TEMPLATE: each field is a "### <label>" heading. Specs use "###"
headings too, so bodies are split only on the known field labels. Spec files
inside the "Spec deltas" field start with "<!-- file: specs/<capability>/spec.md -->".

Standard library only (Python 3.7+); the input of `extract` is untrusted (it comes from an issue).
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys

FIELDS = ["Trace version", "Environment", "Change id", "Exploration", "proposal.md", "Spec deltas", "design.md", "tasks.md", "Checklist"]
CHANGE_ID = re.compile(r"^[a-z0-9][a-z0-9-]{1,63}$")
SPEC_PATH = re.compile(r"^specs/[a-z0-9][a-z0-9/_-]*/spec\.md$")
FILE_MARKER = re.compile(r"^<!--\s*file:\s*(\S+)\s*-->\s*$")
NO_RESPONSE = "_No response_"


def split_fields(body: str) -> dict:
    fields, current, lines = {}, None, []
    for line in body.replace("\r\n", "\n").split("\n"):
        heading = line[4:].strip() if line.startswith("### ") else None
        if heading in FIELDS:
            if current:
                fields[current] = "\n".join(lines).strip()
            current, lines = heading, []
        elif current:
            lines.append(line)
    if current:
        fields[current] = "\n".join(lines).strip()
    return {k: ("" if v == NO_RESPONSE else v) for k, v in fields.items()}


def split_specs(text: str) -> dict:
    files, path, lines = {}, None, []
    for line in text.split("\n"):
        m = FILE_MARKER.match(line.strip())
        if m:
            if path:
                files[path] = "\n".join(lines).strip() + "\n"
            path, lines = m.group(1), []
        elif path:
            lines.append(line)
    if path:
        files[path] = "\n".join(lines).strip() + "\n"
    return {p: c for p, c in files.items() if c.strip()}


def extract(body_file: str, root: str) -> int:
    fields = split_fields(pathlib.Path(body_file).read_text(encoding="utf-8"))
    problems = []
    change_id = fields.get("Change id", "").strip().strip("`")
    if not CHANGE_ID.match(change_id):
        problems.append("**Change id** must be the kebab-case folder name of the change, e.g. `add-funnel-kpi`.")
    for required in ["Exploration", "proposal.md", "tasks.md"]:
        if not fields.get(required):
            problems.append(f"**{required}** is empty.")
    specs = split_specs(fields.get("Spec deltas", ""))
    bad = [p for p in specs if not SPEC_PATH.match(p) or ".." in p]
    if bad:
        problems.append("Spec file paths must look like `specs/<capability>/spec.md`: " + ", ".join(f"`{p}`" for p in bad))
    if not specs:
        problems.append("**Spec deltas** has no files. Start each one with `<!-- file: specs/<capability>/spec.md -->`.")
    if problems:
        print("\n".join(f"- {p}" for p in problems), file=sys.stderr)
        return 2

    change = pathlib.Path(root) / "openspec" / "changes" / change_id
    if change.exists():
        print(f"- A change named `{change_id}` already exists in the repository; pick a new id.", file=sys.stderr)
        return 2
    (change / "specs").mkdir(parents=True)
    (change / "proposal.md").write_text(fields["proposal.md"] + "\n", encoding="utf-8")
    (change / "tasks.md").write_text(fields["tasks.md"] + "\n", encoding="utf-8")
    if fields.get("design.md"):
        (change / "design.md").write_text(fields["design.md"] + "\n", encoding="utf-8")
    for rel, content in specs.items():
        target = (change / rel).resolve()
        if change.resolve() not in target.parents:
            print(f"- `{rel}` points outside the change.", file=sys.stderr)
            return 2
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")
    print(change_id)
    return 0


def build(change_id: str, exploration: str | None) -> int:
    change = pathlib.Path("openspec/changes") / change_id
    if not change.is_dir():
        print(f"No change at {change}. Create one with /opsx:propose first.", file=sys.stderr)
        return 2
    read = lambda name: (change / name).read_text(encoding="utf-8").strip() if (change / name).exists() else ""
    spec_parts = []
    for spec in sorted((change / "specs").rglob("*.md")):
        rel = spec.relative_to(change).as_posix()
        spec_parts.append(f"<!-- file: {rel} -->\n{spec.read_text(encoding='utf-8').strip()}")
    notes = pathlib.Path(exploration).read_text(encoding="utf-8").strip() if exploration else ""
    sections = [
        ("Change id", change_id),
        ("Exploration", notes or "_Summarise what /opsx:explore found: the problem, the files involved and how they behave today._"),
        ("proposal.md", read("proposal.md")),
        ("Spec deltas", "\n\n".join(spec_parts)),
        ("design.md", read("design.md") or NO_RESPONSE),
        ("tasks.md", read("tasks.md")),
        ("Checklist", "- [x] Explored and proposed against the current main\n- [x] `openspec validate --strict` passes\n"
                      "- [x] Privacy and site impact stated\n- [x] No personal data or secrets"),
    ]
    print("\n\n".join(f"### {label}\n\n{value}" for label, value in sections))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("change_id")
    b.add_argument("--exploration", help="file with your /opsx:explore notes")
    e = sub.add_parser("extract")
    e.add_argument("body_file")
    e.add_argument("--root", default=".")
    args = ap.parse_args()
    return build(args.change_id, args.exploration) if args.cmd == "build" else extract(args.body_file, args.root)


if __name__ == "__main__":
    sys.exit(main())
