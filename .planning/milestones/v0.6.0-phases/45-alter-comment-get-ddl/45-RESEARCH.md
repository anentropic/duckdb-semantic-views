# Phase 45: ALTER COMMENT + GET_DDL - Research

**Researched:** 2026-04-11
**Domain:** DuckDB extension DDL (ALTER SET/UNSET COMMENT, GET_DDL round-trip reconstruction)
**Confidence:** HIGH

## Summary

Phase 45 adds two capabilities: (1) ALTER SEMANTIC VIEW SET/UNSET COMMENT to modify view-level comments after creation, and (2) a GET_DDL scalar function that reconstructs re-executable CREATE OR REPLACE SEMANTIC VIEW DDL from the stored JSON definition.

The ALTER COMMENT implementation follows the exact same pattern as the existing ALTER RENAME (Phase 34.1): parser detection in `parse.rs`, statement rewriting to a table function call, a new `AlterCommentVTab` in `ddl/alter.rs`, and persistence via `execute_parameterized`. The comment field already exists in the `SemanticViewDefinition` model (Phase 43).

GET_DDL is a new scalar function using the `VScalar` trait from `duckdb-1.10500.0`. It takes two VARCHAR arguments (`object_type`, `name`), reads the JSON definition from the catalog, and emits a complete `CREATE OR REPLACE SEMANTIC VIEW` statement. The primary complexity is in DDL reconstruction: traversing every field of the model and emitting syntactically correct, parseable DDL, with proper escaping of single quotes in expressions, comments, and synonyms.

**Primary recommendation:** Split into two plans: Plan 1 for ALTER SET/UNSET COMMENT (parser + VTab + persistence), Plan 2 for GET_DDL (scalar function registration + DDL reconstruction + round-trip tests).

## Project Constraints (from CLAUDE.md)

- Quality gate: `just test-all` must pass (Rust tests + sqllogictest + DuckLake CI)
- Build: `just build` for extension binary, `cargo test` for in-memory tests
- `just test-sql` requires fresh `just build`
- If in doubt about SQL syntax, refer to Snowflake semantic views behavior

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| ALT-01 | User can ALTER SEMANTIC VIEW ... SET COMMENT = '...' to add/change view-level comment | Follows ALTER RENAME pattern: new DdlKind variants, parser detection, AlterCommentVTab with catalog read-modify-write + persist |
| ALT-02 | User can ALTER SEMANTIC VIEW ... UNSET COMMENT to remove view-level comment | Same VTab as SET COMMENT with a flag distinguishing set vs unset; sets `comment` to `None` |
| SHOW-07 | GET_DDL('SEMANTIC_VIEW', 'name') returns a re-executable CREATE OR REPLACE statement | New scalar function via VScalar trait; DDL reconstruction from SemanticViewDefinition JSON |
| SHOW-08 | GET_DDL output round-trips correctly (parse -> store -> GET_DDL -> parse produces equivalent definition) | Round-trip test: create view, call GET_DDL, execute the output, compare JSON definitions |
</phase_requirements>

## Architecture Patterns

### Existing ALTER DDL Flow (ALTER RENAME as Reference)

The ALTER RENAME implementation is the template for ALTER SET/UNSET COMMENT. The full flow:

1. **Parser detection** (`parse.rs:detect_ddl_prefix`): `match_keyword_prefix` detects `ALTER SEMANTIC VIEW [IF EXISTS]` keywords
2. **DdlKind mapping**: Currently maps to `AlterRename` / `AlterRenameIfExists`
3. **Validation** (`validate_and_rewrite`): Extracts view name and validates syntax after view name (currently enforces `RENAME TO`)
4. **Rewrite** (`rewrite_ddl`): Converts to `SELECT * FROM alter_semantic_view_rename('old', 'new')`
5. **Table function** (`ddl/alter.rs:AlterRenameVTab`): Reads catalog, validates existence, updates catalog + persistence
6. **Registration** (`lib.rs`): Registers with `register_table_function_with_extra_info` using `AlterRenameState`

[VERIFIED: codebase inspection of `src/parse.rs`, `src/ddl/alter.rs`, `src/lib.rs`]

### Required Changes for ALTER SET/UNSET COMMENT

**Parser changes:**
- Currently, `DdlKind::AlterRename` and `AlterRenameIfExists` are the only ALTER variants
- Must add: `AlterSetComment`, `AlterSetCommentIfExists`, `AlterUnsetComment`, `AlterUnsetCommentIfExists`
- OR: Keep two DdlKind variants (`AlterGeneral`, `AlterGeneralIfExists`) and distinguish SET COMMENT / UNSET COMMENT / RENAME TO in the validation/rewrite phase (simpler DdlKind enum, more complex rewrite logic)
- **Recommended approach:** Add new DdlKind variants for clarity. The enum is internal and small.
- The `validate_and_rewrite` match arm currently errors with "only supports RENAME TO" for non-RENAME ALTER -- this error message and the logic must change to also accept SET COMMENT and UNSET COMMENT

**Detection ordering matters:** The parser uses first-match semantics on `match_keyword_prefix`. The existing `ALTER SEMANTIC VIEW IF EXISTS` (5 keywords) must match before `ALTER SEMANTIC VIEW` (3 keywords). The new SET COMMENT / UNSET COMMENT / RENAME TO distinction happens AFTER prefix detection, in the validation/rewrite logic, not in the prefix matching. So the existing DdlKind structure (AlterRename/AlterRenameIfExists) should be RENAMED to a more general name like `Alter` / `AlterIfExists`, and the specific operation type (RENAME TO, SET COMMENT, UNSET COMMENT) should be determined in `validate_and_rewrite`.

[VERIFIED: codebase inspection of parse.rs line 124-131, 832-865]

**Rewrite format:**
```sql
-- SET COMMENT:
ALTER SEMANTIC VIEW my_view SET COMMENT = 'description'
  -> SELECT * FROM alter_semantic_view_set_comment('my_view', 'description')

-- UNSET COMMENT:
ALTER SEMANTIC VIEW my_view UNSET COMMENT
  -> SELECT * FROM alter_semantic_view_unset_comment('my_view')

-- IF EXISTS variants:
ALTER SEMANTIC VIEW IF EXISTS my_view SET COMMENT = 'desc'
  -> SELECT * FROM alter_semantic_view_set_comment_if_exists('my_view', 'desc')
```

**Table function pattern (AlterCommentVTab):**
1. Read JSON from catalog (as `AlterRenameVTab` does)
2. Deserialize to `SemanticViewDefinition`
3. Modify the `comment` field (set to `Some(value)` or `None`)
4. Re-serialize to JSON
5. Persist (DELETE old row + INSERT new row, or UPDATE)
6. Update in-memory catalog with new JSON
7. Return a result row

[VERIFIED: codebase pattern in `src/ddl/alter.rs` and `src/catalog.rs`]

### Recommended Project Structure Addition

```
src/
  ddl/
    alter.rs            # Extend: add AlterCommentVTab (or AlterSetCommentVTab + AlterUnsetCommentVTab)
    get_ddl.rs          # NEW: GET_DDL scalar function implementation
    mod.rs              # Add: pub mod get_ddl;
  parse.rs              # Modify: DdlKind enum, validate_and_rewrite for SET/UNSET COMMENT
  lib.rs                # Modify: register new table functions + scalar function
test/
  sql/
    phase45_alter_comment.test    # NEW: sqllogictest for ALTER SET/UNSET COMMENT
    phase45_get_ddl.test          # NEW: sqllogictest for GET_DDL round-trip
```

### GET_DDL as a Scalar Function

**Why scalar, not table function:** Snowflake's `GET_DDL` is a scalar function returning VARCHAR. It is called as `SELECT GET_DDL('SEMANTIC_VIEW', 'name')`. A scalar function naturally fits this use case (single input -> single string output). The `VScalar` trait is available in this project's dependency (`duckdb = "=1.10500.0"` with `vscalar` feature enabled). [VERIFIED: Cargo.toml feature flags]

**Implementation pattern:**
```rust
use duckdb::vscalar::{VScalar, ScalarFunctionSignature};
use duckdb::core::{DataChunkHandle, LogicalTypeHandle, LogicalTypeId};
use duckdb::vtab::arrow::WritableVector;
use crate::catalog::CatalogState;

pub struct GetDdlScalar;

impl VScalar for GetDdlScalar {
    type State = CatalogState;

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Read two VARCHAR args: object_type and name
        // Look up name in catalog state
        // Deserialize JSON to SemanticViewDefinition
        // Call render_ddl() to produce CREATE OR REPLACE statement
        // Write result to output vector
    }

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeHandle::from(LogicalTypeId::Varchar),  // object_type
                LogicalTypeHandle::from(LogicalTypeId::Varchar),  // name
            ],
            LogicalTypeHandle::from(LogicalTypeId::Varchar),      // returns DDL string
        )]
    }
}
```

**Registration:**
```rust
con.register_scalar_function_with_state::<GetDdlScalar>(
    "get_ddl",
    &catalog_state,
)?;
```

[VERIFIED: VScalar trait API from `duckdb-1.10500.0/src/vscalar/mod.rs` lines 25-63, 173-193]

### DDL Reconstruction Algorithm

The `render_ddl` function (or `to_ddl` method on `SemanticViewDefinition`) must reconstruct valid DDL from the stored model. This is the most complex part of the phase.

**Output format:**
```sql
CREATE OR REPLACE SEMANTIC VIEW view_name [COMMENT = 'view comment'] AS
TABLES (
    alias1 AS table1 PRIMARY KEY (col1, col2) [UNIQUE (col3)] [COMMENT = 'comment'] [WITH SYNONYMS = ('syn1')],
    alias2 AS table2 PRIMARY KEY (col1)
)
[RELATIONSHIPS (
    rel_name AS from_alias(fk_col1, fk_col2) REFERENCES target_alias
)]
[FACTS (
    [PRIVATE] alias.name AS expr [COMMENT = 'comment'] [WITH SYNONYMS = ('syn1')]
)]
[DIMENSIONS (
    alias.name AS expr [COMMENT = 'comment'] [WITH SYNONYMS = ('syn1')]
)]
[METRICS (
    [PRIVATE] alias.name [USING (rel1, rel2)] AS expr [COMMENT = 'comment'] [WITH SYNONYMS = ('syn1')],
    derived_name AS derived_expr
)]
```

**Model fields to reconstruct:**

| Model Field | DDL Output | Escaping |
|-------------|-----------|----------|
| `base_table` | Not directly emitted -- tables vec used instead | - |
| `tables[].alias` | Identifier before AS | None needed (identifiers) |
| `tables[].table` | After AS keyword | None needed |
| `tables[].pk_columns` | `PRIMARY KEY (col1, col2)` | None needed |
| `tables[].unique_constraints` | `UNIQUE (col1, col2)` per constraint | None needed |
| `tables[].comment` | `COMMENT = 'escaped text'` | Single-quote doubling (`'` -> `''`) |
| `tables[].synonyms` | `WITH SYNONYMS = ('syn1', 'syn2')` | Single-quote doubling |
| `joins[].name` | Relationship name before AS | None needed |
| `joins[].from_alias` | After AS, before `(` | None needed |
| `joins[].fk_columns` | Inside `(col1, col2)` after from_alias | None needed |
| `joins[].table` | After REFERENCES | None needed |
| `dimensions[].name` | After dot in `alias.name` | None needed |
| `dimensions[].source_table` | Before dot in `alias.name` | None needed |
| `dimensions[].expr` | After AS keyword | Must preserve exactly (may contain `'` in strings) |
| `dimensions[].comment` | `COMMENT = '...'` | Single-quote doubling |
| `dimensions[].synonyms` | `WITH SYNONYMS = ('...')` | Single-quote doubling |
| `metrics[].name` | After dot / standalone name | None needed |
| `metrics[].source_table` | Before dot (qualified) or None (derived) | None needed |
| `metrics[].expr` | After AS keyword | Preserve exactly |
| `metrics[].using_relationships` | `USING (rel1, rel2)` before AS | None needed |
| `metrics[].comment` | `COMMENT = '...'` | Single-quote doubling |
| `metrics[].synonyms` | `WITH SYNONYMS = ('...')` | Single-quote doubling |
| `metrics[].access` | `PRIVATE` prefix (only if Private) | None needed |
| `facts[].name` | After dot in `alias.name` | None needed |
| `facts[].source_table` | Before dot | None needed |
| `facts[].expr` | After AS keyword | Preserve exactly |
| `facts[].comment` | `COMMENT = '...'` | Single-quote doubling |
| `facts[].synonyms` | `WITH SYNONYMS = ('...')` | Single-quote doubling |
| `facts[].access` | `PRIVATE` prefix (only if Private) | None needed |
| `comment` (view-level) | `COMMENT = '...'` after view name, before AS | Single-quote doubling |

**Fields NOT reconstructed (internal/runtime):**
- `column_type_names`, `column_types_inferred` -- populated at define time by LIMIT 0 inference
- `created_on` -- timestamp from original creation
- `database_name`, `schema_name` -- context from original creation
- `joins[].on` -- legacy field, not written by Phase 11+ DDL
- `joins[].from_cols` -- legacy field, not written by Phase 11.1+ DDL
- `joins[].join_columns` -- superseded by `fk_columns` + `from_alias`
- `joins[].ref_columns` -- inferred at define time from PK/UNIQUE constraints
- `joins[].cardinality` -- inferred at define time

[VERIFIED: codebase model.rs lines 1-273, body_parser.rs entry parsing functions]

### Escaping Strategy

**Single-quote doubling** is the only escaping needed for DDL string literals:
- Comment strings: `'it''s a test'` -> stored as `it's a test` -> emitted as `'it''s a test'`
- Synonym strings: same treatment
- Expressions: already stored as the raw SQL expression text (no escaping needed -- they are emitted as-is between `AS` and the trailing annotations)

The `extract_view_comment` function in parse.rs already handles `''` -> `'` on input. GET_DDL must do the reverse: `'` -> `''` on output.

[VERIFIED: parse.rs lines 894-907, body_parser.rs parse_trailing_annotations]

### Clause Ordering

The body parser enforces strict clause ordering: TABLES first, then optionally RELATIONSHIPS, FACTS, DIMENSIONS, METRICS. GET_DDL must emit clauses in this same order. Empty optional clauses (no relationships, no facts) should be omitted entirely.

At least one of DIMENSIONS or METRICS must be present (this is enforced by the parser).

[VERIFIED: body_parser.rs CLAUSE_ORDER constant, line 54]

### Anti-Patterns to Avoid

- **Storing DDL text alongside JSON:** Don't store the original DDL text. Reconstruct from the model -- this ensures GET_DDL reflects the current state (e.g., after ALTER SET COMMENT).
- **Separate scalar function per ALTER operation:** Better to share an `AlterCommentState` struct with a `set: bool` flag than to duplicate the VTab implementation. But DO register separate function names for `alter_semantic_view_set_comment` and `alter_semantic_view_unset_comment` (distinct entry points with different parameter counts).
- **Formatting with String concatenation:** Use a `fmt::Write` or `String` builder pattern with `write!()` / `writeln!()` for cleaner, more maintainable DDL emission.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| String escaping in SQL | Custom escape functions | `str.replace('\'', "''")` | SQL single-quote doubling is the only escaping needed; it's a one-liner |
| Scalar function registration | Raw FFI calls | `duckdb::vscalar::VScalar` trait | Handles function set registration, error propagation, memory safety |
| JSON deserialization | Manual field parsing | `serde_json::from_str::<SemanticViewDefinition>` | Already battle-tested in the project's catalog pipeline |

## Common Pitfalls

### Pitfall 1: Catalog Update Atomicity for ALTER COMMENT
**What goes wrong:** If persistence succeeds but in-memory update fails (or vice versa), catalog becomes inconsistent.
**Why it happens:** The ALTER RENAME code does DELETE + INSERT in persistence and a separate catalog_rename in memory. ALTER COMMENT must do a read-modify-write cycle on the JSON.
**How to avoid:** Follow write-first pattern: persist to DB first, then update in-memory catalog. If persist fails, return error before touching memory. Use `catalog_upsert` (which validates JSON) for the in-memory update.
**Warning signs:** Tests that pass with in-memory DB but fail with file-backed DB (or vice versa).

### Pitfall 2: Expression Text Contains Single Quotes
**What goes wrong:** GET_DDL emits `SUM(CASE WHEN status = 'active' THEN 1 ELSE 0 END)` as part of a metric expression. If the expression is not handled carefully, the single quotes inside could break the DDL.
**Why it happens:** Expressions are stored as raw SQL text. They may contain embedded single-quoted strings.
**How to avoid:** Expressions go between `AS` and the trailing annotations in DDL -- they are NOT inside single quotes themselves. They are emitted as-is, verbatim from the stored `expr` field. Only COMMENT and SYNONYM values are single-quoted string literals.
**Warning signs:** Round-trip test failure on views with expressions containing single quotes.

### Pitfall 3: Round-Trip Equivalence Definition
**What goes wrong:** GET_DDL output parsed back produces "equivalent but not identical" JSON because certain fields are populated differently at define time.
**Why it happens:** Fields like `column_type_names`, `column_types_inferred`, `created_on`, `database_name`, `schema_name`, `ref_columns`, and `cardinality` are populated by the define VTab at creation time (e.g., via LIMIT 0 type inference), not by the parser. So parsing GET_DDL output produces a definition without these runtime-populated fields.
**How to avoid:** Define "equivalence" as: same base_table, same tables (alias, table, pk_columns, unique_constraints, comment, synonyms), same joins (name, from_alias, fk_columns, table), same dimensions (name, expr, source_table, comment, synonyms), same metrics (name, expr, source_table, using_relationships, comment, synonyms, access), same facts (name, expr, source_table, comment, synonyms, access), same view-level comment. Ignore runtime-populated fields.
**Warning signs:** Naive JSON string comparison fails even when definitions are logically equivalent.

### Pitfall 4: GET_DDL object_type Validation
**What goes wrong:** User calls `GET_DDL('TABLE', 'my_view')` and gets confusing error or wrong result.
**Why it happens:** Only `SEMANTIC_VIEW` is a valid object type for this extension.
**How to avoid:** Validate that the first argument is exactly `SEMANTIC_VIEW` (case-insensitive). Return a clear error for unsupported types. Snowflake's GET_DDL supports many object types; ours only supports SEMANTIC_VIEW.
**Warning signs:** No error returned for invalid object type.

### Pitfall 5: IF EXISTS Semantics for ALTER COMMENT
**What goes wrong:** `ALTER SEMANTIC VIEW IF EXISTS nonexistent SET COMMENT = 'x'` should be a no-op, but instead errors.
**Why it happens:** Forgetting to check the `if_exists` flag in the new VTab.
**How to avoid:** Copy the if_exists pattern from `AlterRenameVTab::bind()` -- lines 96-99 in `ddl/alter.rs`.
**Warning signs:** Tests for IF EXISTS on non-existent views fail.

### Pitfall 6: Legacy Definitions Without Tables Vec
**What goes wrong:** GET_DDL crashes on old definitions from v0.1.0/v0.2.0 that have empty `tables` vec and only `base_table` populated.
**Why it happens:** Legacy definitions used `base_table` directly without a `tables` array.
**How to avoid:** In DDL reconstruction, handle `tables.is_empty()` case: synthesize a single table entry from `base_table` with empty alias. Or error gracefully with a message suggesting re-creation.
**Warning signs:** GET_DDL on a legacy view panics or produces invalid DDL.

## Code Examples

### ALTER COMMENT VTab Pattern (from AlterRenameVTab)

```rust
// Source: codebase ddl/alter.rs (adapted for SET COMMENT)
pub struct AlterCommentState {
    pub catalog: CatalogState,
    pub persist_conn: Option<ffi::duckdb_connection>,
    pub if_exists: bool,
    pub unset: bool, // true for UNSET COMMENT, false for SET COMMENT
}

impl VTab for AlterSetCommentVTab {
    type BindData = AlterCommentBindData;
    type InitData = AlterCommentInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        bind.add_result_column("status", LogicalTypeHandle::from(LogicalTypeId::Varchar));

        let name = bind.get_parameter(0).to_string();
        let comment = bind.get_parameter(1).to_string(); // SET only
        let state_ptr = bind.get_extra_info::<AlterCommentState>();
        let state = unsafe { &*state_ptr };

        // 1. Read existing JSON from catalog
        let json = {
            let guard = state.catalog.read().unwrap();
            guard.get(&name).cloned()
        };

        match json {
            None if state.if_exists => { /* no-op */ },
            None => return Err(format!("semantic view '{name}' does not exist").into()),
            Some(json_str) => {
                // 2. Deserialize, modify comment, re-serialize
                let mut def: SemanticViewDefinition = serde_json::from_str(&json_str)?;
                def.comment = Some(comment);
                let new_json = serde_json::to_string(&def)?;

                // 3. Persist first
                if let Some(conn) = state.persist_conn {
                    // UPDATE or DELETE+INSERT
                }

                // 4. Update in-memory catalog
                catalog_upsert(&state.catalog, &name, &new_json)?;
            }
        }
        Ok(AlterCommentBindData { name })
    }
    // ... init and func same pattern as AlterRenameVTab
}
```

[VERIFIED: pattern derived from `src/ddl/alter.rs` AlterRenameVTab]

### GET_DDL Scalar Function Pattern

```rust
// Source: duckdb-1.10500.0/src/vscalar/mod.rs test examples
use duckdb::vscalar::{VScalar, ScalarFunctionSignature};

pub struct GetDdlScalar;

impl VScalar for GetDdlScalar {
    type State = CatalogState;

    unsafe fn invoke(
        state: &Self::State,
        input: &mut DataChunkHandle,
        output: &mut dyn WritableVector,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let len = input.len();
        let type_vec = input.flat_vector(0);
        let name_vec = input.flat_vector(1);
        let types = type_vec.as_slice_with_len::<duckdb_string_t>(len);
        let names = name_vec.as_slice_with_len::<duckdb_string_t>(len);
        let output = output.flat_vector();

        for i in 0..len {
            let obj_type = DuckString::new(&mut { types[i] }).as_str().to_string();
            let name = DuckString::new(&mut { names[i] }).as_str().to_string();

            if !obj_type.eq_ignore_ascii_case("SEMANTIC_VIEW") {
                return Err(format!("GET_DDL: unsupported object type '{obj_type}'. Only 'SEMANTIC_VIEW' is supported.").into());
            }

            let guard = state.read().unwrap();
            let json = guard.get(&name).ok_or_else(|| {
                format!("semantic view '{name}' does not exist")
            })?;
            let def: SemanticViewDefinition = serde_json::from_str(json)?;
            let ddl = render_create_ddl(&name, &def);
            output.insert(i, ddl.as_str());
        }
        Ok(())
    }

    fn signatures() -> Vec<ScalarFunctionSignature> {
        vec![ScalarFunctionSignature::exact(
            vec![
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            ],
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        )]
    }
}
```

[VERIFIED: VScalar trait API from duckdb-1.10500.0]

### DDL Reconstruction Helper

```rust
fn render_create_ddl(name: &str, def: &SemanticViewDefinition) -> String {
    let mut out = String::new();
    out.push_str("CREATE OR REPLACE SEMANTIC VIEW ");
    out.push_str(name);

    // View-level comment
    if let Some(ref c) = def.comment {
        out.push_str(" COMMENT = '");
        out.push_str(&c.replace('\'', "''"));
        out.push('\'');
    }

    out.push_str(" AS\nTABLES (\n");
    // ... emit tables
    out.push_str(")\n");

    if !def.joins.is_empty() {
        out.push_str("RELATIONSHIPS (\n");
        // ... emit relationships
        out.push_str(")\n");
    }

    if !def.facts.is_empty() {
        out.push_str("FACTS (\n");
        // ... emit facts
        out.push_str(")\n");
    }

    if !def.dimensions.is_empty() {
        out.push_str("DIMENSIONS (\n");
        // ... emit dimensions
        out.push_str(")\n");
    }

    if !def.metrics.is_empty() {
        out.push_str("METRICS (\n");
        // ... emit metrics
        out.push_str(")\n");
    }

    out
}
```

### Annotation Emission Helper

```rust
fn emit_comment(out: &mut String, comment: &Option<String>) {
    if let Some(ref c) = comment {
        out.push_str(" COMMENT = '");
        out.push_str(&c.replace('\'', "''"));
        out.push('\'');
    }
}

fn emit_synonyms(out: &mut String, synonyms: &[String]) {
    if !synonyms.is_empty() {
        out.push_str(" WITH SYNONYMS = (");
        for (i, s) in synonyms.iter().enumerate() {
            if i > 0 { out.push_str(", "); }
            out.push('\'');
            out.push_str(&s.replace('\'', "''"));
            out.push('\'');
        }
        out.push(')');
    }
}
```

## Snowflake Alignment

| Feature | Snowflake | This Extension | Notes |
|---------|-----------|---------------|-------|
| ALTER SET COMMENT | `ALTER SEMANTIC VIEW [IF EXISTS] name SET COMMENT = 'str'` | Same syntax | [CITED: docs.snowflake.com/en/sql-reference/sql/alter-semantic-view] |
| ALTER UNSET COMMENT | `ALTER SEMANTIC VIEW [IF EXISTS] name UNSET COMMENT` | Same syntax | [CITED: docs.snowflake.com/en/sql-reference/sql/alter-semantic-view] |
| ALTER RENAME | `ALTER SEMANTIC VIEW [IF EXISTS] name RENAME TO new_name` | Already implemented (Phase 34.1) | - |
| GET_DDL | `SELECT GET_DDL('SEMANTIC_VIEW', 'name')` | Same syntax | Scalar function returning VARCHAR |
| GET_DDL return | Returns CREATE statement text | Returns CREATE OR REPLACE statement | OR REPLACE ensures idempotent re-execution |
| GET_DDL namespace | Supports `'db.schema.name'` qualified names | Single-part names only | Extension does not yet have multi-catalog support |
| GET_DDL PRIVATE visibility | PRIVATE items included in output | Same | [CITED: docs.snowflake.com/en/user-guide/views-semantic/sql] |

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `ALTER SEMANTIC VIEW` only supports RENAME TO | Phase 45 adds SET/UNSET COMMENT | Phase 45 | Parser error message must be updated |
| No DDL reconstruction capability | GET_DDL scalar function | Phase 45 | First scalar function in the extension |
| All DDL ops use table functions | GET_DDL uses scalar function | Phase 45 | New registration pattern |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | sqllogictest-rs + cargo test (Rust unit/proptests) |
| Config file | `test/sql/TEST_LIST` for sqllogictest |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| ALT-01 | ALTER SET COMMENT modifies comment visible in SHOW/DESCRIBE | integration (slt) | `just test-sql` | No - Wave 0 |
| ALT-02 | ALTER UNSET COMMENT removes comment | integration (slt) | `just test-sql` | No - Wave 0 |
| SHOW-07 | GET_DDL returns valid CREATE OR REPLACE statement | integration (slt) + unit | `just test-sql` + `cargo test` | No - Wave 0 |
| SHOW-08 | GET_DDL round-trips correctly | integration (slt) | `just test-sql` | No - Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase45_alter_comment.test` -- covers ALT-01, ALT-02
- [ ] `test/sql/phase45_get_ddl.test` -- covers SHOW-07, SHOW-08
- [ ] Unit tests for `render_create_ddl()` in `ddl/get_ddl.rs` -- covers SHOW-07, SHOW-08
- [ ] Unit tests for ALTER parsing in `parse.rs` -- covers ALT-01, ALT-02
- [ ] TEST_LIST update to include new .test files

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | GET_DDL should be a scalar function (not a table function) matching Snowflake's interface | Architecture Patterns | Low - table function would also work but scalar is more natural for single-value return |
| A2 | GET_DDL should use `CREATE OR REPLACE` (not plain `CREATE`) for idempotent re-execution | Snowflake Alignment | Low - `CREATE OR REPLACE` is safer and matches Snowflake behavior |
| A3 | Legacy definitions (empty tables vec) should produce an error in GET_DDL rather than attempting reconstruction | Common Pitfalls | Medium - could instead attempt best-effort reconstruction |
| A4 | The `render_create_ddl` function should live in a new `ddl/get_ddl.rs` module | Architecture Patterns | Low - organizational choice, could also go in model.rs |

## Open Questions

1. **Should GET_DDL handle legacy definitions?**
   - What we know: Definitions from v0.1.0/v0.2.0 may have empty `tables` vec and only `base_table`
   - What's unclear: Whether any such definitions still exist in practice
   - Recommendation: Return a clear error ("Legacy definition format; please re-create the view using CREATE OR REPLACE") rather than attempting reconstruction. This is safe because any user with a legacy definition can re-create it easily.

2. **Should DdlKind variants be refactored for ALTER?**
   - What we know: Currently `AlterRename` / `AlterRenameIfExists` are specific to RENAME
   - What's unclear: Whether to add 4 new variants or refactor to general `Alter` / `AlterIfExists` with sub-dispatch
   - Recommendation: Rename existing variants to `Alter` / `AlterIfExists` and add sub-operation detection in `validate_and_rewrite`. This avoids enum bloat (would need 6 variants for 3 ALTER operations x 2 IF EXISTS variants) and makes future ALTER extensions easier.

3. **Should GET_DDL support qualified names (db.schema.name)?**
   - What we know: Snowflake supports `GET_DDL('SEMANTIC_VIEW', 'db.schema.view')` for cross-database access
   - What's unclear: Whether this extension needs cross-database support
   - Recommendation: Support single-part names only for now. Multi-part names are a future enhancement when cross-catalog support is added.

## Sources

### Primary (HIGH confidence)
- Codebase inspection: `src/parse.rs`, `src/ddl/alter.rs`, `src/model.rs`, `src/body_parser.rs`, `src/catalog.rs`, `src/lib.rs`
- `duckdb-1.10500.0` crate: `src/vscalar/mod.rs` (VScalar trait, registration API, test examples)

### Secondary (MEDIUM confidence)
- [Snowflake ALTER SEMANTIC VIEW docs](https://docs.snowflake.com/en/sql-reference/sql/alter-semantic-view) - SET/UNSET COMMENT syntax and semantics
- [Snowflake GET_DDL docs](https://docs.snowflake.com/en/sql-reference/functions/get_ddl) - Function signature and SEMANTIC_VIEW support
- [Snowflake semantic views SQL guide](https://docs.snowflake.com/en/user-guide/views-semantic/sql) - PRIVATE items in GET_DDL output

## Metadata

**Confidence breakdown:**
- ALTER COMMENT implementation: HIGH - direct extension of existing ALTER RENAME pattern with identical architecture
- GET_DDL scalar function: HIGH - VScalar trait is well-documented with test examples in the crate
- DDL reconstruction: MEDIUM - reconstruction logic must handle all model fields correctly; edge cases around legacy definitions and complex expressions need thorough testing
- Round-trip correctness: MEDIUM - depends on exact alignment between parser input handling and GET_DDL output formatting

**Research date:** 2026-04-11
**Valid until:** 2026-05-11 (stable codebase, no expected dependency changes)
