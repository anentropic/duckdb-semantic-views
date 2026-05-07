# Page Inventory -- Phase 62 Content Refresh (v0.8.0)

**Generated:** 2026-05-06
**Mode:** Content refresh only (no new pages)
**Branch:** milestone/v0.8.0
**Trigger:** Phase 62 of the v0.8.0 milestone landed today, resolving TECH-DEBT 22 (caret rendering loss) and TECH-DEBT 20 (16-DB LRU eviction). Pre-existing WIP working-tree changes correctly document Phases 58/60/61 but predate Phase 62 and use "v0.8.1" framing for a milestone that was consolidated into v0.8.0 on 2026-05-05 and never tagged.

This is a narrow refresh run. All entries below layer onto the existing working-tree WIP changes -- none replace WIP content wholesale.

---

## Change Summary

Phase 62 produced two structural changes that invalidate documentation written during Phases 59-61:

1. **Caret rendering restored** (TECH-DEBT 22). Phase 59 unified DDL through `parser_override`, which delivered errors as bare runtime `Invalid Input Error` exceptions (no `LINE 1: ... ^` annotation). Phase 62 re-introduced `parse_function` purely as the error-reporting layer with `OverrideContext` attached to `SemanticViewsParserInfo`. Validation errors now render as `Parser Error: ... LINE 1: ... ^` again, matching the historical pre-extension behaviour.

2. **Bounded 16-DB LRU removed** (TECH-DEBT 20). Phase 61 introduced a 16-entry insertion-order LRU with explicit eviction errors. Phase 62 deleted the LRU entirely; multi-database isolation is now unbounded with lifetimes tied to `DBConfig`. The "catalog context for this database has been evicted (process has opened more than 16 databases)" error is unreachable.

3. **Milestone consolidation** (administrative). Working drafts split the milestone into v0.8.0 + v0.8.1 (4851e43, d6f077d). On 2026-05-05 the work was consolidated into a single v0.8.0; v0.8.1 was never tagged. Doc passages that say "Since v0.8.1" must normalise to "Since v0.8.0".

---

## Content Refresh

| # | Type | Title | Key Sections / Proposed Edits | File Path | Scope |
|---|------|-------|-------------------------------|-----------|-------|
| 1 | (refresh) | Transactional DDL and Known Limitations | **Substantive -- two section removals + intro/summary reconciliation + framing pass.** (a) Lines 49-74 ("Error Messages Are Now Runtime Errors", `_explanation-txn-ddl-error-formatting`): remove the entire section including the `.. versionchanged:: 0.8.1` directive. Phase 62 restored the parse-time caret via `parse_function` as the error-reporting layer, so the rendering is no longer surprising and there is no "limitation" left to explain. (b) Lines 150-162 ("Long-Running Processes Across Many Databases", `_explanation-txn-ddl-multi-db`): remove the entire section. Phase 62 deleted the bounded LRU; the eviction error is unreachable. (c) Line 14 (intro paragraph): remove the phrases "daemons that open dozens of databases" and "One change (error formatting) is visible everywhere, and is purely cosmetic" -- both reference the now-removed sections. Tighten the surrounding sentence so the intro still scans. (d) Lines 187-202 ("Summary"): remove the bullet "Validation errors arrive as runtime ``Invalid Input Error`` exceptions without the caret-style annotation. The error text is unchanged." (~line 195) and the bullet "A single program opening more than 16 databases will see eviction errors on the oldest one." (~line 201). The remaining bullets (transactional CREATE/DROP/ALTER, read-committed introspection, concurrent CREATE IF NOT EXISTS, PEG parser) all stay. (e) "See also" list at lines 204-209: no edits required, all targets still exist. (f) Cross-reference label `_explanation-txn-ddl-error-formatting` will be deleted alongside section (a) -- inbound link from `docs/reference/error-messages.rst` is handled in entry #2. (g) Verify no other "v0.8.1" / "0.8.1" references remain after sections (a) is removed -- the removal also takes lines 54, 58, 66 with it. | docs/explanation/transactional-ddl-and-limitations.rst | substantive |
| 2 | (refresh) | Error Messages | **Minor -- single note rewrite + retarget cross-reference.** Lines 12-14 admonition currently warns: "Since v0.8.1, DDL validation errors (``semantic view 'X' does not exist``, unknown clauses, name uniqueness violations, and so on) arrive as runtime ``Invalid Input Error`` exceptions rather than parse-time errors with caret-position formatting. The error text is unchanged; only the rendering and source-line annotation are different. See :ref:`explanation-txn-ddl-error-formatting` for the mechanism." This is wrong post-Phase-62 (caret is back) and points at a label that entry #1 removes. Recommended action: **remove the admonition entirely** (lines 12-14, plus the surrounding blank lines). The error catalogue below the note already documents each error verbatim; a "rendering changed" warning is no longer needed. Optionally, before the first error subsection, add a single short paragraph: "Validation errors render as DuckDB ``Parser Error`` with the standard ``LINE 1: ... ^`` caret annotation. The examples below show the bare error text; the runtime output additionally includes the offending source line." If kept, do not add a `:ref:` cross-reference to the removed label. | docs/reference/error-messages.rst | minor |
| 3 | (refresh) | ALTER SEMANTIC VIEW | **Minor -- one-token framing edit.** Line 52 admonition contains "Since v0.8.1, the non-``IF EXISTS`` forms additionally raise ``semantic view '<name>' was concurrently dropped`` ...". Change "Since v0.8.1" to "Since v0.8.0". The behaviour itself is correct (Phase 60 added the concurrent-drop guard); only the version label is stale. No other edits needed; the rest of the admonition matches current source. | docs/reference/alter-semantic-view.rst | minor |
| 4 | (refresh) | DROP SEMANTIC VIEW | **Minor -- one-token framing edit.** Line 36 admonition contains "Since v0.8.1, ``DROP SEMANTIC VIEW`` (without ``IF EXISTS``) additionally raises ``semantic view '<name>' was concurrently dropped`` ...". Change "Since v0.8.1" to "Since v0.8.0". Same rationale as entry #3 -- Phase 60 behaviour is correct, version label is stale. | docs/reference/drop-semantic-view.rst | minor |

---

## Pages Verified -- No Changes Needed (pre-deselected)

These pages were flagged by the timestamp heuristic but read against current source code and the WIP working tree show no genuine drift. Exclude from the approved write set.

| # | Type | Title | Reason | File Path |
|---|------|-------|--------|-----------|
| -- | (refresh) -- no changes needed | CREATE SEMANTIC VIEW | WIP admonitions added in working tree (Phase 58 transactional + IF NOT EXISTS race caveat) are accurate against post-Phase-62 source. Both notes use "Since v0.8.0" framing already; no "v0.8.1" present. | docs/reference/create-semantic-view.rst |
| -- | (refresh) -- no changes needed | DESCRIBE SEMANTIC VIEW | WIP read-committed visibility note is accurate. Targets `:ref:`explanation-txn-ddl-write-visibility`` which is preserved by entry #1. No "v0.8.1" framing. | docs/reference/describe-semantic-view.rst |
| -- | (refresh) -- no changes needed | SHOW SEMANTIC VIEWS | WIP read-committed visibility note is accurate. Targets `:ref:`explanation-txn-ddl-write-visibility`` which is preserved by entry #1. No "v0.8.1" framing. | docs/reference/show-semantic-views.rst |
| -- | (refresh) -- no changes needed | YAML Definitions (how-to) | WIP "Since v0.8.0 ... FROM YAML FILE participates in the caller's transaction" note is accurate. No "v0.8.1" framing. | docs/how-to/yaml-definitions.rst |
| -- | (refresh) -- no changes needed | Explanation index | WIP toctree update for transactional-ddl-and-limitations is accurate. The page itself stays in the toctree post-refresh; only two sections are removed from the page. | docs/explanation/index.rst |
| -- | (refresh) -- no changes needed | Remaining 35 pages (Snowflake/Databricks comparison, YAML format reference, materialization routing, getting-started, all other reference pages, all how-to guides) | Timestamp-stale only. Cover features unrelated to Phase 62 changes. Spot-checked against current source -- no drift. | docs/**/*.rst (remainder) |

---

## API Reference Status

Manual API reference (per `.doc-writer/config.yaml`: `api_reference: manual`). No auto-generated reference. This project exposes a SQL interface and reference pages are hand-authored SQL syntax pages. Inline `code` mentions in this refresh use plain literal formatting without links, consistent with existing pages. No new symbol mentions are introduced by the proposed edits.

## Audience Targeting

All four refresh entries target the single configured persona: **Data engineers exploring semantic views** (intermediate). The `never_assume` list includes "duckdb-semantic-views DDL syntax" and "Differences from Snowflake/Databricks behavior" -- both already respected by surrounding prose in each affected page. Removing the caret-loss and 16-DB sections actually improves persona calibration: those passages explained behaviour the user will never observe, which would have confused them on first read.

## Coverage Gaps

None introduced. The two sections being removed from `transactional-ddl-and-limitations.rst` describe behaviours that no longer exist in v0.8.0 source as of Phase 62. Removing them does not create a documentation gap -- there is nothing left to document about either case.

## Build-Output Note (informational)

`docs/_build/html/...` and `docs/_build/html/_sources/...` contain stale "v0.8.1" framing too (artefacts of an earlier `sphinx-build` run). These regenerate from source on the next build. Not part of this refresh.

## Cross-Cutting "v0.8.1" Verification

Fresh grep at inventory time:

```
grep -rn "v0\.8\.1\|0\.8\.1" docs/ --include="*.rst" | grep -v "_build"
```

returns 6 matches across 4 files -- all covered by the entries above:

- `docs/explanation/transactional-ddl-and-limitations.rst` lines 54, 58, 66 -- handled by entry #1 (sections containing them are removed)
- `docs/reference/error-messages.rst` line 14 -- handled by entry #2
- `docs/reference/alter-semantic-view.rst` line 52 -- handled by entry #3
- `docs/reference/drop-semantic-view.rst` line 36 -- handled by entry #4

No "v0.8.1" reference is missed by this inventory.

---

## Summary of Work

- **0 new pages** to write
- **4 pages** requiring content refresh
  - 1 substantive (transactional-ddl-and-limitations.rst -- two section removals + intro/summary reconciliation)
  - 3 minor (single-line edits in error-messages.rst, alter-semantic-view.rst, drop-semantic-view.rst)
- **40+ pages** confirmed as still accurate (no changes needed; pre-deselected for approval prompt)

## Out-of-Scope (per delegation message)

- Internal Rust `pub` items (vtable structs, bind data types, parser functions) are not user-facing -- no API reference work.
- New v0.8.0 features already documented correctly in WIP (transactional CREATE/DROP/ALTER, IF NOT EXISTS race, FROM YAML FILE transactions, read-committed DESCRIBE/SHOW visibility) are preserved as-is.
- The 39 timestamp-stale-but-content-current pages are not refreshed.
