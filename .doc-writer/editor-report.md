# Editor Report

**Generated:** 2026-03-25
**Files reviewed:** 11
**Changes made:** 3
  - BLOCKING: 0
  - SUGGESTION: 3
  - NITPICK: 0

## Summary

`docs/explanation/snowflake-comparison.rst` is well-structured and technically accurate overall. Two targeted fixes were applied to the page: the schema-scope limitation for automatic PK resolution was made explicit (the list-table and a new note admonition), and the internal HTML comment block was stripped. The explanation index description was also corrected to remove a misleading reference to Cortex Analyst.

---

## docs/explanation/snowflake-comparison.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Key Differences / Primary Key Declarations | The list-table's "Catalog PK available?" cell read "Yes" for native DuckDB tables with a `PRIMARY KEY` constraint, implying automatic resolution for all native tables. Source code at `src/ddl/define.rs` line 114 confirms the catalog query hard-codes `schema_name = 'main'`, so tables in non-default schemas are not resolved automatically. The Author's notes flagged this as a known limitation. | Changed the cell to "Yes (main schema only)" and added a `.. note::` admonition below the table stating that automatic resolution applies only to the `main` schema. |
| Post-processing | Internal HTML comment block (lines 287-310, `<!-- INTERNAL NOTES FOR EDITOR ... -->`) was present in the source file and needed to be stripped before publication. | Removed the entire comment block and collapsed trailing blank lines. |

---

## docs/explanation/index.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Page list, line 10 | Description read: "Feature-by-feature comparison with Snowflake Cortex Analyst semantic views and where this extension diverges." The phrase "Cortex Analyst semantic views" is inaccurate: the comparison page targets Snowflake's SQL DDL interface only, not Cortex Analyst. The comparison page itself opens with a `.. note::` admonition clarifying this distinction, and `context.md` explicitly states all comparisons must target the SQL DDL interface. | Changed to: "Feature-by-feature comparison with Snowflake's SQL DDL semantic views and where this extension diverges." |

---

## Pass Results by Pass

### Pass 1: Terminology Consistency

Scanned all documentation files in `docs/` (excluding `.venv`). No terminology variants requiring normalization were found in `snowflake-comparison.rst` or any other scanned file. All occurrences of proper nouns (DuckDB, Snowflake, Iceberg, Parquet, Postgres, Cortex Analyst) and project terms (semantic view, derived metric, base table, fan trap) matched canonical forms from `terminology.yaml`. No updates to `terminology.yaml` required.

### Pass 2: Diataxis Type Integrity

Page is classified as `explanation` (per Author's internal notes, reference used: `skills/diataxis/references/explanation.md`). Content is correctly understanding-oriented: it maps concepts between platforms, calls out behavioral differences, and explains the reasoning behind design choices (e.g., why explicit `PRIMARY KEY` is needed for external sources). No blur signals detected.

The illustrative code examples in the Primary Key Declarations section are appropriate for explanation (they show two cases side by side to build understanding, not instruct through a sequential task). The "Features Not Yet Supported" table informs scope understanding rather than directing action. No changes required on the target page.

One cross-type finding in `docs/explanation/index.rst`: the page description conflated the SQL DDL interface with Cortex Analyst, contradicting the comparison page's explicit scope. Fixed (see above).

### Pass 3: Humanizer

No AI-writing patterns detected in `snowflake-comparison.rst`. The prose is direct and professional with no em dashes in prose, significance inflation, copula avoidance, filler phrases, or AI vocabulary. No changes required.

### Pass 4: Cross-Reference Linking

`api_reference` is set to `manual` in `config.yaml`. All inline code mentions in prose that refer to public API symbols are already linked via `:ref:` syntax:

- `:ref:\`semantic_view() <ref-semantic-view-function>\`` -- already linked (appears in Concept Mapping table, Query Interface section, and Features Not Yet Supported table)
- `:ref:\`fan trap detection <howto-fan-traps>\`` -- already linked in Cardinality Inference section

No unlinked public API symbols found in prose. Pass complete with no changes to the target file.

---

## Terminology Changes

No terminology changes required. Existing term map in `.doc-writer/terminology.yaml` is consistent with all scanned documentation files.
