---
created: 2026-05-15T13:48:41.554Z
title: Remove obsolete pre-0.5.5 backwards-compatibility shims
area: general
files:
  - src/catalog.rs:18
  - src/catalog.rs:34-60
---

## Problem

v0.5.5 was the first release published to the DuckDB community-extensions registry. Any code path that exists solely to migrate from or interoperate with pre-0.5.5 on-disk state is dead — no real user could ever hit it. Carrying this code costs review time on every catalog change and complicates the read-only LOAD path (Phase 63), where it has to be explicitly skipped.

Known candidates:

- **v0.1.0 companion-file migration** (`src/catalog.rs:18`, `34-60`): `init_catalog` constructs a sidecar path (`<db>.<ext>.semantic_views`), reads it as JSON, INSERT-OR-REPLACE's its contents into `semantic_layer._definitions`, and deletes the file. The `V010_COMPANION_EXT` constant exists only for this. Safe to drop entirely along with the constant.

Audit while in there:

- Other `*_v01*` / `*_legacy*` / "migration" / "companion" references across `src/`.
- Any conditional behavior keyed on schema version that predates v0.5.5.
- `Cargo.toml` / extension descriptor entries that exist for old loaders.

## Solution

TBD — straightforward deletion plus an audit pass. Should land as its own small phase (or a `/gsd-quick`) with:

1. Grep audit to enumerate every pre-0.5.5 shim.
2. Delete code + constants + any tests that exercise the dead path.
3. Confirm `just test-all` stays green (no test should depend on the companion-file migration).
4. CHANGELOG note under `Removed` for the version this lands in.
