# Gap Report

**Generated:** 2026-05-06
**Source root:** src/
**Language:** rust
**Total undocumented symbols:** 0
**Potentially stale pages:** 4 (with concrete content discrepancies)

## Undocumented Symbols

No undocumented user-facing features. The Rust `pub` items identified by the export scanner are internal implementation details (vtable structs, bind data types, parser functions) and are not part of the user-facing SQL interface that this documentation set targets.

Previous gap report (2026-04-26) also found 0 undocumented symbols.

## Potentially Stale Pages

The v0.8.0 milestone (just completed, 2026-05-06) introduced two limitations mid-milestone (TECH-DEBT 22 caret loss, TECH-DEBT 20 bounded LRU) that were RESOLVED structurally in Phase 62. Existing documentation describes these as live limitations and must be refreshed.

Additionally, the working tree contains 8 modified `.rst` files + 1 new file from a prior doc-writer WIP session that **correctly** documents the post-Phase-58/60/61 state but predates Phase 62. Those WIP changes should be preserved; Phase 62 only mandates four narrow refreshes on top of them.

### Real refresh candidates

- `docs/reference/error-messages.rst` (doc: 2026-05-03, source: 2026-05-06) — Line 14 says: "Since v0.8.1, DDL validation errors ... arrive as runtime Invalid Input Error exceptions rather than parse-time errors with caret-position formatting." This is now incorrect: Phase 62 (TECH-DEBT 22) restored caret rendering via `parse_function` as the error-reporting layer. Validation errors now produce `Parser Error: ... LINE 1: ... ^` again. Also the "v0.8.1" framing is stale — that version was never tagged; the milestone was consolidated to v0.8.0.

- `docs/explanation/transactional-ddl-and-limitations.rst` (untracked, new file from prior WIP) — Two passages need refresh:
  - **Lines 56–74 (caret loss):** describes validation errors arriving without caret as a v0.8.0 limitation with the line "If DuckDB ever fixes the quirk, the caret will return on its own." Phase 62 fixed this in-extension (not in DuckDB), so the limitation should be removed from this page entirely or explicitly marked as resolved in v0.8.0.
  - **Lines 155–159, 201 (16-DB LRU eviction):** describes the 16-database LRU as a runtime limitation users may hit. Phase 62 (TECH-DEBT 20) deleted the bounded LRU; multi-DB isolation is now unbounded (lifetime-tied to `DBConfig`). This entire section should be removed or replaced with a "no longer applies" note.

- `docs/reference/error-messages.rst` (continued) — Once caret restoration is documented, may also benefit from a small example showing the restored `LINE 1: ... ^` annotation in error output.

- `docs/explanation/transactional-ddl-and-limitations.rst` (continued) — The page references "v0.8.1" framing throughout (e.g., "Since v0.8.1"). All such references should be normalized to "v0.8.0" since v0.8.1 was never tagged (the milestone was consolidated 2026-05-05).

### Cross-cutting "v0.8.1" → "v0.8.0" normalization

`grep -rn "v0\.8\.1\|0\.8\.1" docs/` to find every reference. From quick inspection: at least these files use the v0.8.1 framing:

- `docs/reference/alter-semantic-view.rst` (line 50: "Since v0.8.1, the non-IF EXISTS forms additionally raise...")
- `docs/reference/error-messages.rst` (line 14: "Since v0.8.1, DDL validation errors...")
- Possibly more.

Author should grep all docs and decide which references to update to v0.8.0 vs which to keep (none should remain — v0.8.1 was never released).

### NOT stale (pre-existing WIP — preserve as-is)

The following pages were modified in a prior doc-writer WIP session and correctly reflect Phases 58/60/61. They are timestamp-newer than most other pages and should be preserved:

- `docs/explanation/index.rst` — toc update for transactional-ddl page
- `docs/how-to/yaml-definitions.rst` — Phase 58 transactional FROM YAML FILE note
- `docs/reference/alter-semantic-view.rst` — Phase 58 transactional + Phase 60 concurrent-drop guard (only the v0.8.1 → v0.8.0 framing needs touch-up)
- `docs/reference/create-semantic-view.rst` — Phase 58 transactional + IF NOT EXISTS race caveat
- `docs/reference/describe-semantic-view.rst` — read-committed visibility note
- `docs/reference/drop-semantic-view.rst` — Phase 58 transactional DROP note
- `docs/reference/show-semantic-views.rst` — read-committed visibility note

### Timestamp-stale but content-current

39 of 41 pages were modified before today's Phase 62 commits. Most cover features unrelated to v0.8.0 (YAML definitions, materialization routing, Snowflake comparison, etc.) and need no refresh. The Author should verify by content comparison; the load-bearing pages are the four listed above.

## Notes

- The timestamp heuristic flags every page as stale (Phase 62 modified `src/` today). The 4 specific refresh candidates above are the only ones with identified content discrepancies vs the post-Phase-62 source state.
- Pre-existing WIP changes in working tree are intentional and must NOT be overwritten — the Author should diff against `HEAD` to see them.
- This is a "narrow refresh" run, not a full rewrite. Expected output: ~6 small content edits across 2 files plus a project-wide v0.8.1 → v0.8.0 framing normalization.
