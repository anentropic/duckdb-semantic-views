# Phase 22: Documentation - Research

**Researched:** 2026-03-09
**Domain:** README documentation update (DDL syntax reference)
**Confidence:** HIGH

## Summary

Phase 22 is a documentation-only phase: update the README to replace function-based syntax examples with the new native DDL syntax and add a DDL reference section. No code changes are required. The current README (163 lines) documents v0.4.0 function-based syntax (`create_semantic_view()`, `drop_semantic_view()`, etc.) and needs to be updated to show the native DDL equivalents introduced in v0.5.0 and extended in v0.5.1.

The ground truth for DDL syntax is the SQL logic test files (`test/sql/phase16_parser.test`, `test/sql/phase20_extended_ddl.test`). These demonstrate all 7 DDL forms, their exact syntax, case insensitivity, and error behavior. The README version string also needs updating from v0.4.0 to the current version.

**Primary recommendation:** Replace the "Creating a semantic view" and "Other DDL functions" sections with native DDL syntax. Add a lifecycle worked example. Keep the querying, explain, and building sections largely unchanged. Retain function-based syntax as an alternative (documented briefly) since backward compatibility is maintained.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Keep it simple -- match the current level of detail in the README
- Replace existing function syntax examples with DDL equivalents
- Show all new DDL verbs
- Do NOT add comprehensive documentation -- that comes in a later phase
- Copy the tone and detail level already in the README
- No over-documentation

### Claude's Discretion
- Exact section ordering and headings
- Whether to keep function syntax as an alternative or replace entirely
- Wording of examples

### Deferred Ideas (OUT OF SCOPE)
- Comprehensive documentation (dedicated future phase)
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DOC-01 | README includes DDL syntax reference with worked examples | Current README fully analyzed; all 7 DDL forms documented from test files; exact syntax patterns extracted; lifecycle example pattern available from `phase20_extended_ddl.test` |
</phase_requirements>

## Current README Analysis

**Confidence:** HIGH (direct source analysis)

The current README has these sections that need updating:

| Section | Lines | Status | Action |
|---------|-------|--------|--------|
| Title + intro | 1-8 | Needs version bump | Update `v0.4.0` to current version |
| How it works | 10-16 | OK | No changes needed |
| Loading | 18-30 | OK | No changes needed |
| Creating a semantic view | 32-95 | **Replace** | Rewrite with native DDL syntax |
| Querying | 97-128 | OK | No changes needed (query interface unchanged) |
| Explain | 130-142 | OK | No changes needed |
| Other DDL functions | 144-153 | **Replace** | Expand into DDL reference section with all 7 verbs |
| Building | 155-168 | OK | No changes needed |
| License | 170-173 | OK | No changes needed |

### Current Tone and Detail Level

The README uses:
- Short section headings (2-3 words)
- One explanatory sentence followed by a code block
- Real SQL examples with realistic table/column names (`orders`, `region`, `amount`)
- No API-doc-style tables or formal specifications
- Casual but technical tone ("The extension figures out the SQL")
- Both single-table and multi-table examples for create

## DDL Syntax Reference (Ground Truth)

**Confidence:** HIGH (extracted from passing SQL logic tests)

All 7 DDL forms, verified working in `test/sql/phase20_extended_ddl.test`:

### 1. CREATE SEMANTIC VIEW

```sql
CREATE SEMANTIC VIEW view_name (
    tables := [{'alias': 'alias', 'table': 'table_name'}],
    dimensions := [{'name': 'dim_name', 'expr': 'expression', 'source_table': 'alias'}],
    metrics := [{'name': 'metric_name', 'expr': 'agg_expression', 'source_table': 'alias'}]
);
```

### 2. CREATE OR REPLACE SEMANTIC VIEW

```sql
CREATE OR REPLACE SEMANTIC VIEW view_name (
    tables := [...],
    dimensions := [...],
    metrics := [...]
);
```

### 3. CREATE SEMANTIC VIEW IF NOT EXISTS

```sql
CREATE SEMANTIC VIEW IF NOT EXISTS view_name (
    tables := [...],
    dimensions := [...],
    metrics := [...]
);
```

### 4. DROP SEMANTIC VIEW

```sql
DROP SEMANTIC VIEW view_name;
```

### 5. DROP SEMANTIC VIEW IF EXISTS

```sql
DROP SEMANTIC VIEW IF EXISTS view_name;
```

### 6. DESCRIBE SEMANTIC VIEW

Returns 6 columns: name, base_table, dimensions, metrics, filters, joins.

```sql
DESCRIBE SEMANTIC VIEW view_name;
```

### 7. SHOW SEMANTIC VIEWS

Returns 2 columns: name, base_table.

```sql
SHOW SEMANTIC VIEWS;
```

### Syntax Details

- **Case insensitive:** All DDL keywords are case insensitive (`create semantic view`, `CREATE SEMANTIC VIEW`, `Create Or Replace Semantic View` all work)
- **Clause arguments:** Use DuckDB's `:=` named parameter syntax with struct/list literals
- **Required clauses:** `tables` and at least one of `dimensions` or `metrics`
- **Optional clauses:** `relationships` (for multi-table joins)
- **View names:** Unquoted identifiers (no schema qualification -- flat namespace)

### Multi-table Syntax (with relationships)

```sql
CREATE SEMANTIC VIEW order_analytics (
    tables := [
        {'alias': 'o', 'table': 'orders'},
        {'alias': 'c', 'table': 'customers'}
    ],
    relationships := [
        {'from_table': 'o', 'to_table': 'c',
         'join_columns': [{'from': 'customer_id', 'to': 'id'}]}
    ],
    dimensions := [
        {'name': 'region', 'expr': 'region', 'source_table': 'o'},
        {'name': 'customer_tier', 'expr': 'tier', 'source_table': 'c'}
    ],
    metrics := [
        {'name': 'revenue', 'expr': 'sum(amount)', 'source_table': 'o'}
    ]
);
```

## Architecture Patterns

### Recommended README Structure

The updated README should follow this structure (matching current tone):

```
# DuckDB Semantic Views
  [intro paragraph -- updated version]

## How it works
  [unchanged]

## Loading
  [unchanged]

## Defining a semantic view
  [NEW: native DDL syntax with single-table and multi-table examples]

## Querying
  [unchanged]

## DDL reference
  [NEW: all 7 DDL verbs with one-liner + code block each]

## Lifecycle example
  [NEW: create -> query -> describe -> drop worked example]

## Explain
  [unchanged or folded into DDL reference]

## Function syntax (alternative)
  [brief note that function-based DDL still works, with minimal examples]

## Building
  [unchanged]

## License
  [unchanged]
```

### Key Discretion Decision: Keep Function Syntax as Alternative

**Recommendation:** Keep function syntax documented briefly as a secondary alternative. Rationale:
1. The test files confirm backward compatibility is maintained (`phase20_extended_ddl.test` lines 159-168)
2. MAINTAINER.md examples still use function syntax for the Python worked example
3. Removing all mention of it would confuse users who find references to it in issues/code
4. A brief "Function syntax" subsection (3-4 lines) costs almost nothing

### Lifecycle Example Pattern

From `phase20_extended_ddl.test` lines 257-305, the lifecycle test provides the exact pattern:

1. CREATE a semantic view
2. Query it with `semantic_view()`
3. DESCRIBE it
4. SHOW SEMANTIC VIEWS to list all
5. DROP it

The README example should follow this pattern but use realistic data (like the existing `orders` table) rather than test fixture names.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| DDL syntax examples | Inventing syntax from memory | Copy from `test/sql/phase20_extended_ddl.test` | Tests are the ground truth; they pass CI |
| Version number | Guessing the current version | Read from `Cargo.toml` (`version = "0.5.0"`) | Single source of truth |

## Common Pitfalls

### Pitfall 1: Stale function syntax in examples
**What goes wrong:** Updating the "Creating" section but leaving function syntax references in other sections (explain, querying)
**Why it happens:** The README has cross-references between sections
**How to avoid:** Review every code block in the README for consistency
**Warning signs:** Any `SELECT * FROM create_semantic_view(` still present in the main flow

### Pitfall 2: Wrong version string
**What goes wrong:** README says v0.4.0 but the extension is v0.5.0+
**Why it happens:** Version was bumped in Cargo.toml but not in README
**How to avoid:** Update the version string in the intro paragraph
**Warning signs:** `v0.4.0` still present in the README

### Pitfall 3: Over-documentation
**What goes wrong:** Adding detailed API tables, formal BNF grammar, or comprehensive parameter descriptions
**Why it happens:** Natural tendency to document everything
**How to avoid:** CONTEXT.md explicitly says "match the current level of detail" and "no over-documentation"
**Warning signs:** README grows beyond ~200 lines; formal syntax diagrams appear

### Pitfall 4: Inconsistent struct field formatting
**What goes wrong:** Using different field names or ordering than what the extension actually expects
**Why it happens:** The struct fields have specific names (`alias`, `table`, `name`, `expr`, `source_table`, `from_table`, `to_table`, `join_columns`, `from`, `to`)
**How to avoid:** Copy struct literals from passing test files
**Warning signs:** Examples that don't match the test file patterns

### Pitfall 5: Forgetting the semicolons
**What goes wrong:** DDL examples missing trailing semicolons
**Why it happens:** README code blocks sometimes omit them for brevity
**How to avoid:** All DDL examples in the test files use semicolons; follow that pattern
**Warning signs:** Copy-paste from README into DuckDB fails

## Code Examples

Verified patterns from SQL logic test files (ground truth):

### Single-table CREATE (from phase16_parser.test)
```sql
CREATE SEMANTIC VIEW sales_view (
    tables := [{'alias': 'sales', 'table': 'sales'}],
    dimensions := [{'name': 'region', 'expr': 'region', 'source_table': 'sales'}],
    metrics := [{'name': 'total_amount', 'expr': 'SUM(amount)', 'source_table': 'sales'}]
);
```

### CREATE OR REPLACE (from phase20_extended_ddl.test)
```sql
CREATE OR REPLACE SEMANTIC VIEW sv_replace_test (
    tables := [{'alias': 'sv_test', 'table': 'sv_test'}],
    dimensions := [{'name': 'region', 'expr': 'region', 'source_table': 'sv_test'}],
    metrics := [{'name': 'avg_amount', 'expr': 'AVG(amount)', 'source_table': 'sv_test'}]
);
```

### CREATE IF NOT EXISTS (from phase20_extended_ddl.test)
```sql
CREATE SEMANTIC VIEW IF NOT EXISTS sv_idempotent_test (
    tables := [{'alias': 'sv_test', 'table': 'sv_test'}],
    dimensions := [{'name': 'region', 'expr': 'region', 'source_table': 'sv_test'}],
    metrics := [{'name': 'total_amount', 'expr': 'SUM(amount)', 'source_table': 'sv_test'}]
);
```

### DROP and DROP IF EXISTS (from phase20_extended_ddl.test)
```sql
DROP SEMANTIC VIEW sv_replace_test;
DROP SEMANTIC VIEW IF EXISTS sv_nonexistent;
```

### DESCRIBE (from phase20_extended_ddl.test)
```sql
DESCRIBE SEMANTIC VIEW desc_test;
-- Returns: name, base_table, dimensions (JSON), metrics (JSON), filters (JSON), joins (JSON)
```

### SHOW (from phase20_extended_ddl.test)
```sql
SHOW SEMANTIC VIEWS;
-- Returns: name, base_table
```

### Query interface (unchanged from current README)
```sql
SELECT * FROM semantic_view(
    'orders',
    dimensions := ['region', 'category'],
    metrics := ['revenue', 'order_count']
);
```

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest (DuckDB runner) + cargo test |
| Config file | Makefile (test-sql target) |
| Quick run command | `just test-sql` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DOC-01 | README includes DDL syntax reference with worked examples | manual-only | N/A -- documentation review | N/A |

**Justification for manual-only:** DOC-01 is a documentation requirement. The content accuracy is verified by cross-referencing with the existing SQL logic tests (which already pass). There is no automated way to test "README is readable and complete." The verification step should confirm:
1. All 7 DDL verbs appear in the README with examples
2. At least one lifecycle example (create, query, describe, drop)
3. Every SQL code block in the README is syntactically valid (can be verified by visual inspection against test files)
4. `just test-all` still passes (no code changes, but confirms nothing was accidentally broken)

### Sampling Rate
- **Per task commit:** Visual review of README changes
- **Per wave merge:** `just test-all` (confirm no regressions)
- **Phase gate:** README review + `just test-all` green

### Wave 0 Gaps
None -- this is a documentation-only phase with no test infrastructure requirements.

## Open Questions

1. **Should the version string be updated to v0.5.0 or v0.5.1?**
   - What we know: Cargo.toml says `version = "0.5.0"`. The current milestone is v0.5.1 but it is not yet tagged.
   - Recommendation: Update to v0.5.0 (the last tagged release). When v0.5.1 ships, the version will be bumped in a separate step.

2. **Should `explain_semantic_view()` get a DDL equivalent?**
   - What we know: The test files show no `EXPLAIN SEMANTIC VIEW` DDL form. Explain remains function-based only.
   - Recommendation: Document it as a function (current approach). No DDL equivalent exists.

## Sources

### Primary (HIGH confidence)
- `README.md` -- current state of documentation (163 lines)
- `test/sql/phase20_extended_ddl.test` -- all 7 DDL forms with exact syntax (312 lines)
- `test/sql/phase16_parser.test` -- original CREATE SEMANTIC VIEW syntax (105 lines)
- `test/sql/phase21_error_reporting.test` -- error behavior (116 lines)
- `Cargo.toml` -- version = "0.5.0"
- `.planning/phases/22-documentation/22-CONTEXT.md` -- user constraints

### Secondary (MEDIUM confidence)
- `MAINTAINER.md` -- architecture overview and worked examples (shows function syntax still in use)
- `TECH-DEBT.md` -- decision #12 confirms all-VARCHAR DDL output (relevant for DESCRIBE/SHOW column descriptions)

## Metadata

**Confidence breakdown:**
- DDL syntax: HIGH -- extracted directly from passing test files
- README structure: HIGH -- direct analysis of current file
- Pitfalls: HIGH -- derived from concrete current-state analysis
- Version handling: MEDIUM -- current milestone not yet tagged

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (documentation structure stable; DDL syntax frozen for v0.5.1)
