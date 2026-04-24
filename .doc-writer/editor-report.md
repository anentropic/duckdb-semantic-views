# Editor Report

**Generated:** 2026-04-22
**Files reviewed:** 31
**Changes made:** 7
  - BLOCKING: 1
  - SUGGESTION: 3
  - NITPICK: 3

## Summary

The documentation is in strong shape. Prose is clean with no AI writing patterns, no em dashes, no chatbot artifacts, and no promotional language detected across all 31 RST files. Terminology is consistent with the canonical forms in terminology.yaml. The primary findings are: one accuracy mismatch in the error-messages reference page, a categorization issue in reference/index.rst, capitalization inconsistencies with "non-additive" in prose, and minor "DDL-time" vs "define-time" wording inconsistency. Cross-references are complete and all :ref: targets resolve correctly.

---

## reference/error-messages.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| YAML size limit exceeded (line 399-401) | Accuracy mismatch with source code. The documented error format shows `(<size> bytes > <cap> bytes)` but the actual format in `src/model.rs:486-488` is `({size} bytes > {cap} byte cap)`. The source appends "byte cap" after the cap number, while the docs show "bytes". | Update the error message template in the docs from `(<size> bytes > <cap> bytes)` to `(<size> bytes > <cap> byte cap)` to match the source at `src/model.rs:486-488`. |

### SUGGESTION

(none)

### NITPICK

(none)

---

## reference/index.rst

### BLOCKING

(none)

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| DDL statements list (lines 14-27) | Categorization issue: three entries listed under "DDL statements" are not DDL statements. `ref-get-ddl` (GET_DDL) is a scalar function. `ref-read-yaml` (READ_YAML_FROM_SEMANTIC_VIEW) is a scalar function. `ref-yaml-format` (YAML Definition Format) is a format specification. Grouping these under "DDL statements" is misleading for readers scanning the page to find a specific type of reference. | Consider adding a separate heading between "DDL statements" and "Query functions" for these three items. For example, an "Export / import" or "Utility functions" group containing GET_DDL, READ_YAML_FROM_SEMANTIC_VIEW, and the YAML format spec. This better reflects their nature and makes the page easier to scan. |

### NITPICK

(none)

---

## how-to/semi-additive-metrics.rst

### BLOCKING

(none)

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Troubleshooting (line 189) | Inconsistent capitalization: "Non-additive" appears mid-sentence with capital N twice: "If all Non-additive dimensions are in the query" and "Remove the Non-additive dimension from the query". In running prose (not headings or bold labels starting a line), this should be lowercase "non-additive" to match the terminology map and standard English conventions. | Change both mid-sentence occurrences of "Non-additive" to "non-additive" on line 189. |
| Troubleshooting (line 191) | Inconsistent capitalization in bold label: "Non-Additive" uses Title Case ("Performance with multiple Non-Additive dimension sets"). Other troubleshooting bold labels in this file and across other how-to pages use sentence case. | Change to "non-additive" in the bold label: `**Performance with multiple non-additive dimension sets**`. |

### NITPICK

(none)

---

## reference/read-yaml-from-semantic-view.rst

### BLOCKING

(none)

### SUGGESTION

(none)

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Field Stripping table (lines 63-66) | Two descriptions use "DDL-time" as an adjective: "Column name list from DDL-time type inference" and "Column type IDs from DDL-time type inference." Every other page in the docs uses "define time" or "define-time" for this concept. | Consider changing "DDL-time" to "define-time" in both rows for consistency with the rest of the docs. |

---

## Pass-by-Pass Findings

### Pass 1: Terminology Consistency

No terminology inconsistencies found beyond the "non-additive" capitalization issue noted above. All API symbol names (`semantic_view()`, `explain_semantic_view()`, `READ_YAML_FROM_SEMANTIC_VIEW()`, `GET_DDL()`, etc.) are used consistently across all files. Proper nouns (DuckDB, Snowflake, Databricks, Iceberg, Parquet, Postgres) are correctly capitalized throughout. Project-specific terms (semantic view, base table, derived metric, role-playing dimension, fan trap, materialization) are used consistently.

The `duckdb-sql` vs `sql` code-block language distinction is intentional and correct: `duckdb-sql` is used only for code blocks containing dollar-quoting syntax (which the standard SQL lexer cannot handle), while `sql` is used everywhere else.

### Pass 2: Diataxis Type Integrity

All pages maintain their declared types without significant blur:

- **Tutorials** (getting-started, multi-table): Clean learning-oriented structure with step-by-step progression, expected output, verification steps, and "What You Learned" summaries. No explanation digressions or option overload.
- **How-to guides** (all 12 pages): Goal-oriented with prerequisites, focused task instructions, and troubleshooting sections. The fan-traps page has a brief "What Is a Fan Trap?" section that borders on explanation, but it is concise context necessary for the procedural content and links to the explanation page for details. Acceptable.
- **Explanation pages** (3 pages): Understanding-oriented with concept mapping, comparison tables, and architectural context. No embedded step-by-step instructions. Code examples are illustrative, not instructional.
- **Reference pages** (17 pages): Information-oriented with consistent Syntax/Parameters/Output/Examples structure following the Snowflake SQL reference page pattern. No tutorial-style teaching or how-to framing.

No structural blur requiring page splits was detected.

### Pass 3: Humanizer

No AI writing patterns detected across all 31 files. Specifically confirmed absent:
- Em dashes (Unicode or triple-hyphen): none found
- Chatbot artifacts: none found
- Sycophantic tone: none found
- Significance inflation: none found
- AI vocabulary overuse (delve, enhance, leverage, comprehensive, etc.): none found
- Copula avoidance (serves as, stands as, etc.): none found
- Filler phrases (In order to, It is important to note, etc.): none found
- Excessive hedging: none found
- Promotional language (robust, powerful, seamless, etc.): none found
- Curly quotation marks: none found

The prose reads as clean, professional technical documentation.

### Pass 4: Cross-Reference Linking

The `api_reference` config field is set to `"manual"`, indicating manually written SQL reference pages rather than auto-generated API docs. Since this is a Rust/C++ DuckDB extension with a SQL interface (no Python/JS API), there are no domain-specific roles (`:py:func:`, `:js:class:`) to validate.

All inline code mentions of public API symbols in running prose are linked via `:ref:` to their reference pages. Spot-checked across all how-to guides, tutorials, and explanation pages. No unlinked mentions of `semantic_view()`, `explain_semantic_view()`, `READ_YAML_FROM_SEMANTIC_VIEW()`, `GET_DDL()`, `DESCRIBE SEMANTIC VIEW`, `SHOW SEMANTIC VIEWS`, `SHOW SEMANTIC DIMENSIONS`, `SHOW SEMANTIC METRICS`, `SHOW SEMANTIC FACTS`, `SHOW SEMANTIC MATERIALIZATIONS`, `SHOW COLUMNS IN SEMANTIC VIEW`, `CREATE SEMANTIC VIEW`, `ALTER SEMANTIC VIEW`, or `DROP SEMANTIC VIEW` were found.

All `:ref:` targets in the newly added pages (yaml-definitions.rst, yaml-format.rst, read-yaml-from-semantic-view.rst, show-semantic-materializations.rst, materializations.rst) were verified against their label definitions. No broken references found.

---

## Terminology Changes

| Term | Before | After | Authority |
|------|--------|-------|-----------|
| Non-additive (mid-sentence prose) | "Non-additive" | "non-additive" | English convention: lowercase compound adjective in running prose |
| Non-Additive (bold label) | "Non-Additive" | "non-additive" | English convention: lowercase compound adjective in running prose |
| DDL-time (adjective) | "DDL-time" | "define-time" | Project terminology map: "define time" is the canonical form |
