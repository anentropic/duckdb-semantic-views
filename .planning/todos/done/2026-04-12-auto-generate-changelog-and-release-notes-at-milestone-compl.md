---
created: 2026-04-12T10:21:17.789Z
title: Auto-generate CHANGELOG and release notes at milestone completion
area: tooling
files: []
---

## Problem

CHANGELOG.md is not being maintained. There is no step in the milestone completion workflow that generates or updates release notes. Each milestone ships multiple phases with numerous commits, but no human-readable summary of changes exists for end users or contributors.

## Solution

Add an end-of-milestone task (ideally as part of `/gsd-complete-milestone` or a standalone skill) that:

1. Parses git log between the previous tag and current HEAD
2. Groups changes by category (features, fixes, breaking changes, internal)
3. Generates a CHANGELOG.md entry for the milestone version
4. Optionally generates GitHub release notes format

Could use `git log --oneline v0.5.5..HEAD` as input, with commit prefixes (feat/fix/docs/test/chore) for categorization. Consider conventional-changelog tooling or a custom GSD skill.
