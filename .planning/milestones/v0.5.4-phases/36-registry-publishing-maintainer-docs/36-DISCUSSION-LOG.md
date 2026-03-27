# Phase 36: Registry Publishing & Maintainer Docs - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md -- this log preserves the alternatives considered.

**Date:** 2026-03-27
**Phase:** 36-registry-publishing-maintainer-docs
**Areas discussed:** description.yml content, CE submission strategy, MAINTAINER.md rewrite scope, Milestone close tasks

---

## GitHub Org/Username

| Option | Description | Selected |
|--------|-------------|----------|
| anentropic | Matches conf.py, README, and docs site URL | :heavy_check_mark: |
| paul-rl | The username in the current MAINTAINER.md template | |

**User's choice:** anentropic
**Notes:** All other project references (conf.py, README, docs site) use `anentropic`.

---

## Hello World Example

| Option | Description | Selected |
|--------|-------------|----------|
| Single-table DDL + query | Simple CREATE SEMANTIC VIEW with one table, 1-2 dims, 1-2 metrics, then query | :heavy_check_mark: |
| Multi-table with relationships | Two tables with PK/FK, shows JOIN inference | |
| Minimal metric-only | Smallest possible, one table, one metric, one dimension | |

**User's choice:** Single-table DDL + query
**Notes:** Quick to understand, validates the full pipeline.

---

## CE Submission Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| Submit draft PR early | Find out if build pipeline handles C++ amalgamation before final submission | :heavy_check_mark: |
| Local validation first | Try to replicate CE build locally before submitting | |
| Just submit and iterate | Submit real PR and fix as issues arise | |

**User's choice:** Submit draft PR early
**Notes:** Hybrid Rust+C++ build pipeline is untested. Draft PR is lowest-risk discovery approach.

---

## MAINTAINER.md Rewrite Scope

| Option | Description | Selected |
|--------|-------------|----------|
| Targeted updates only | Fix username, update examples, add multi-branch + CE update sections. Keep accurate sections as-is | :heavy_check_mark: |
| Full rewrite | Rewrite entire document | |
| Append only | Just add new sections, leave stale content | |

**User's choice:** Targeted updates only
**Notes:** Prerequisites, Quick Start, Architecture, Testing, Fuzzing, CI sections are still accurate.

---

## End-of-Milestone Python Example

| Option | Description | Selected |
|--------|-------------|----------|
| Snowflake-parity features | UNIQUE constraints, cardinality inference, ALTER RENAME, SHOW commands with filtering | :heavy_check_mark: |
| Full feature showcase | Everything from v0.5.3 + v0.5.4 | |
| You decide | Claude picks best scope | |

**User's choice:** Snowflake-parity features
**Notes:** Covers v0.5.4 headline features specifically.

---

## Claude's Discretion

- Exact description.yml extended_description wording
- MAINTAINER.md section ordering and formatting
- Python example file structure and data setup
- Whether to include CE submission workflow recipe

## Deferred Ideas

None -- all discussion stayed within phase scope.
