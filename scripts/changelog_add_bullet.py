#!/usr/bin/env python3
"""Insert a bullet under the ``## [Unreleased]`` -> ``### <category>`` section.

Used by the DuckDB Version Monitor workflow so an automated DuckDB-pin bump also
records itself in the changelog, in the project's Keep-a-Changelog format, under
the ``[Unreleased]`` section (the convention documented in CLAUDE.md).

Behaviour:
- Idempotent: if the bullet text already appears anywhere in the Unreleased
  block, nothing is written (safe across the monitor's weekly force-push reruns).
- Replaces the ``_No unreleased changes yet._`` placeholder when present.
- Creates the ``### <category>`` subheading in canonical Keep-a-Changelog order
  if it does not exist yet, otherwise appends to the existing one.

Usage:
    changelog_add_bullet.py <Category> <bullet text without leading "- ">

Exit status is always 0 on success (including the idempotent no-op).
"""

import sys

CHANGELOG = "CHANGELOG.md"
# Canonical Keep a Changelog 1.1.0 ordering; only these are allowed as ### heads.
CATEGORIES = ["Added", "Changed", "Deprecated", "Removed", "Fixed", "Security"]
PLACEHOLDER = "_No unreleased changes yet._"


def normalize(block):
    """Collapse repeated blank lines; pad with exactly one blank line each side."""
    out = []
    for line in block:
        if line.strip() == "" and (not out or out[-1].strip() == ""):
            continue
        out.append(line)
    while out and out[0].strip() == "":
        out.pop(0)
    while out and out[-1].strip() == "":
        out.pop()
    return [""] + out + [""]


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: changelog_add_bullet.py <Category> <bullet text>")
    category, bullet = sys.argv[1], sys.argv[2]
    if category not in CATEGORIES:
        sys.exit(f"category must be one of {CATEGORIES}, got {category!r}")

    lines = open(CHANGELOG, encoding="utf-8").read().split("\n")

    try:
        start = next(i for i, l in enumerate(lines) if l.strip() == "## [Unreleased]")
    except StopIteration:
        sys.exit("could not find '## [Unreleased]' heading in CHANGELOG.md")
    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i].startswith("## [")),
        len(lines),
    )
    block = lines[start + 1 : end]

    bullet_line = f"- {bullet}"
    if any(bullet.strip() in l for l in block):
        print("changelog: bullet already present; no change")
        return

    block = [l for l in block if l.strip() != PLACEHOLDER]

    cat_head = f"### {category}"
    head_idx = next((i for i, l in enumerate(block) if l.strip() == cat_head), None)
    if head_idx is not None:
        # Append after the last contiguous bullet of this subsection.
        last = head_idx
        for k in range(head_idx + 1, len(block)):
            if block[k].startswith("- "):
                last = k
            elif block[k].startswith("### "):
                break
        block.insert(last + 1, bullet_line)
    else:
        # Create the subheading in canonical order relative to existing ones.
        rank = {c: i for i, c in enumerate(CATEGORIES)}
        mine = rank[category]
        insert_at = len(block)
        for i, l in enumerate(block):
            s = l.strip()
            if s.startswith("### "):
                other = s[4:].strip()
                if rank.get(other, 999) > mine:
                    insert_at = i
                    break
        block[insert_at:insert_at] = ["", cat_head, "", bullet_line]

    new_lines = lines[: start + 1] + normalize(block) + lines[end:]
    open(CHANGELOG, "w", encoding="utf-8").write("\n".join(new_lines))
    print(f"changelog: added under [Unreleased] ### {category}: {bullet}")


if __name__ == "__main__":
    main()
