# Gap Detection Report

**Source root:** src/
**Language:** rust
**Docs root:** docs/ (reStructuredText, Sphinx + Shibuya — not Markdown)
**Generated:** 2026-08-09, against version 0.12.0 (unreleased)

## Headline

| Surface | Total | Documented | Undocumented |
|---------|-------|------------|--------------|
| **User-facing SQL surface** (DDL statements + registered SQL functions) | 21 | 21 | 0 |
| Internal Rust `pub` items (`scan-exports.sh` output) | 165 | 0 (by design) | 165 (not applicable) |

The raw `scan-exports.sh` number is **not a meaningful coverage signal for this
project**. `duckdb-semantic-views` builds a `cdylib` DuckDB extension; its public
contract is SQL (`CREATE SEMANTIC VIEW`, `semantic_view(...)`, `SHOW SEMANTIC …`),
not a Rust API. Nothing under `src/` is consumed by users as a crate, so the 165
`pub` items are internal module boundaries and correctly absent from `docs/`. The
table below is the coverage check that actually matters.

## User-Facing SQL Surface — Coverage

### DDL statements

| Statement | Reference page | Status |
|-----------|----------------|--------|
| `CREATE SEMANTIC VIEW` (+ `OR REPLACE`, `IF NOT EXISTS`, `FROM YAML`) | `reference/create-semantic-view.rst` | documented |
| `DROP SEMANTIC VIEW [IF EXISTS]` | `reference/drop-semantic-view.rst` | documented |
| `ALTER SEMANTIC VIEW … RENAME TO` | `reference/alter-semantic-view.rst` | documented |
| `ALTER SEMANTIC VIEW … SET COMMENT` | `reference/alter-semantic-view.rst` | documented |
| `ALTER SEMANTIC VIEW … UNSET COMMENT` | `reference/alter-semantic-view.rst` | documented |
| `DESCRIBE SEMANTIC VIEW` | `reference/describe-semantic-view.rst` | documented |
| `SHOW SEMANTIC VIEWS` | `reference/show-semantic-views.rst` | documented |
| `SHOW SEMANTIC DIMENSIONS` | `reference/show-semantic-dimensions.rst` | documented |
| `SHOW SEMANTIC DIMENSIONS … FOR METRIC` | `reference/show-semantic-dimensions-for-metric.rst` | documented |
| `SHOW SEMANTIC METRICS` | `reference/show-semantic-metrics.rst` | documented |
| `SHOW SEMANTIC FACTS` | `reference/show-semantic-facts.rst` | documented |
| `SHOW SEMANTIC MATERIALIZATIONS` | `reference/show-semantic-materializations.rst` | documented |
| `SHOW COLUMNS IN SEMANTIC VIEW` | `reference/show-columns-semantic-view.rst` | documented |

The `ALTER` grammar in `src/parse/rewrite.rs:205` ("Supported: RENAME TO, SET
COMMENT, UNSET COMMENT") matches `reference/alter-semantic-view.rst` exactly — no
`ADD`/`DROP <member>` forms exist, and their absence from the docs is correct, not
a gap.

### Registered SQL functions

| Function | Documented in | Status |
|----------|---------------|--------|
| `semantic_view(...)` | `reference/semantic-view-function.rst` + 29 other pages | documented |
| `explain_semantic_view(...)` | `reference/explain-semantic-view-function.rst` + 12 pages | documented |
| `get_ddl(...)` (both arities) | `reference/get-ddl.rst` + 9 pages | documented |
| `read_yaml_from_semantic_view(...)` | `reference/read-yaml-from-semantic-view.rst` + 8 pages | documented |

### `semantic_view()` named parameters

Every named parameter accepted in `cpp/src/shim.cpp` — `dimensions`, `metrics`,
`facts`, `where_clause`, `search_path` — appears in the docs (`where_clause` in 4
pages, `search_path` in 5). No parameter is undocumented.

### DDL clause features

Spot-checked against the parser; all present in `docs/`: `TABLES`,
`RELATIONSHIPS`, `FACTS`, `DIMENSIONS`, `METRICS`, `PRIMARY KEY`, `UNIQUE`,
`REFERENCES`, `USING RELATIONSHIPS`, `COMMENT =`, `WITH SYNONYMS`,
`NON ADDITIVE BY`, `OVER` (window metrics), `MATERIALIZATION`, `FROM YAML`.

## Undocumented Exports

No undocumented **user-facing** exports. Two lower-confidence items are worth a
deliberate decision rather than an automatic fix:

| Symbol | File | Type | Note |
|--------|------|------|------|
| `list_semantic_views`, `list_terse_semantic_views` | `cpp/src/shim.cpp` | table function | Backs `SHOW SEMANTIC VIEWS`. Registered, therefore directly callable by a user, but never named in the docs. |
| `show_semantic_dimensions_all`, `show_semantic_metrics_all`, `show_semantic_facts_all`, `show_semantic_materializations_all` | `cpp/src/shim.cpp` | table function | Arity variants backing the un-scoped `SHOW SEMANTIC …` forms. Same situation. |
| `describe_semantic_view`, `show_columns_in_semantic_view` | `cpp/src/shim.cpp` | table function | Function-form of the corresponding `DESCRIBE` / `SHOW COLUMNS` statements. Documented only in statement form. |

These are implementation plumbing that DuckDB's function registry happens to
expose. Documenting them would invite users onto an unsupported surface; the
current omission is defensible. The gap is that nothing in the docs *says* they
are unsupported. Either is fine — it just shouldn't be accidental.

## Notes

- **Docs are RST, not Markdown.** `paths.docs_root: "docs/"` in
  `.doc-writer/config.yaml` still records `detected.doc_system: "plain-markdown"`
  and `existing_docs: false`, both stale — the project is Sphinx + Shibuya with 41
  RST pages plus `changelog.md`. Worth correcting in config so future
  `/doc-writer` runs glob the right extensions.
- **Docs track the code closely.** Ten `.. versionadded:: 0.12.0` /
  `versionchanged:: 0.12.0` directives already exist for the unreleased version,
  covering per-grain aggregation, fan traps, facts, and the Snowflake comparison.
  Doc commits are interleaved with feature commits (e.g. `06e95c8`, `ae03567`,
  `bca6e91`), so drift is being caught at PR time rather than accumulating.
- **`docs/.venv/` and `docs/_build/` pollute naive globs.** A `docs/**/*.md`
  scan returns mostly vendored package READMEs. Any tooling pointed at this repo
  should exclude both.
- Coverage was checked by matching each registered name and DDL keyword against
  `docs/**/*.rst`, case-insensitively, excluding `.venv` and `_build`.
