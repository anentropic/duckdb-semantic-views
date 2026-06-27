#!/usr/bin/env python3
"""Roll the ``## [Unreleased]`` section into a tagged version section.

Used by the publish workflow at release time. Moves everything currently under
``## [Unreleased]`` into a new ``## [<version>] - <date>`` section, resets
Unreleased to the placeholder, and fixes the link-reference block at the bottom:

- ``[Unreleased]`` now compares ``v<version>...HEAD``
- a new ``[<version>]`` line compares ``<prev_tag>...v<version>``

The compare-URL base is read from the existing ``[Unreleased]:`` reference, so
nothing about the repo URL is hardcoded here.

Usage:
    changelog_release.py <version> <date YYYY-MM-DD> <prev_tag e.g. v0.10.3>

Refuses (exit 1) if the Unreleased section has no real content — you should not
cut a release with an empty changelog.
"""

import re
import sys

CHANGELOG = "CHANGELOG.md"
PLACEHOLDER = "_No unreleased changes yet._"


def main():
    if len(sys.argv) != 4:
        sys.exit("usage: changelog_release.py <version> <date> <prev_tag>")
    version, date, prev_tag = sys.argv[1], sys.argv[2], sys.argv[3]

    lines = open(CHANGELOG, encoding="utf-8").read().split("\n")

    start = next(
        (i for i, l in enumerate(lines) if l.strip() == "## [Unreleased]"), None
    )
    if start is None:
        sys.exit("could not find '## [Unreleased]' heading")
    end = next(
        (i for i in range(start + 1, len(lines)) if lines[i].startswith("## [")),
        len(lines),
    )

    body = [l for l in lines[start + 1 : end] if l.strip()]
    if not body or all(l.strip() == PLACEHOLDER for l in body):
        sys.exit("refusing to release: [Unreleased] has no content")

    # Trim leading/trailing blanks from the moved body.
    moved = lines[start + 1 : end]
    while moved and moved[0].strip() == "":
        moved.pop(0)
    while moved and moved[-1].strip() == "":
        moved.pop()

    new_unreleased = [
        "## [Unreleased]",
        "",
        PLACEHOLDER,
        "",
        f"## [{version}] - {date}",
        "",
        *moved,
        "",
    ]
    lines = lines[:start] + new_unreleased + lines[end:]

    # Update the link-reference block.
    ur_idx = next(
        (i for i, l in enumerate(lines) if l.startswith("[Unreleased]:")), None
    )
    if ur_idx is None:
        sys.exit("could not find '[Unreleased]:' link reference")
    m = re.match(r"(\[Unreleased\]:\s*)(\S+?)/compare/\S+\.\.\.HEAD\s*$", lines[ur_idx])
    if not m:
        sys.exit(f"unexpected [Unreleased] link format: {lines[ur_idx]!r}")
    base = m.group(2)  # e.g. https://github.com/anentropic/duckdb-semantic-views
    lines[ur_idx] = f"[Unreleased]: {base}/compare/v{version}...HEAD"
    lines.insert(
        ur_idx + 1,
        f"[{version}]: {base}/compare/{prev_tag}...v{version}",
    )

    open(CHANGELOG, "w", encoding="utf-8").write("\n".join(lines))
    print(f"changelog: released [{version}] - {date} (compare {prev_tag}...v{version})")


if __name__ == "__main__":
    main()
