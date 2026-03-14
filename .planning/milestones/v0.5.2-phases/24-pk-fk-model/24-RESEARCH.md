# Phase 24: PK/FK Model - Research

**Researched:** 2026-03-09
**Domain:** Rust model structs, serde serialization, DuckDB function parameter types
**Confidence:** HIGH

## Summary

Phase 24 extends the existing `model.rs` data structures to support Snowflake-style PK/FK relationship declarations. This is a pure data model phase -- no parser changes, no expansion changes, no SQL generation changes. The goal is to make the `SemanticViewDefinition` struct capable of storing primary keys per table, FK-based relationships (with optional names), and source table aliases on dimensions/metrics parsed from qualified `alias.name` syntax.

The existing model already has partial support: `TableRef` stores `alias` and `table`, `Join` stores `table` and `join_columns`, and `Dimension`/`Metric` have `source_table`. What is missing: (1) PK columns on `TableRef`, (2) a `from_alias` field on `Join` (currently relationships only store the target table, not the source alias), (3) an optional relationship name, and (4) the function-based DDL path (`create_semantic_view()`) needs its DuckDB type signatures updated to accept PK columns and relationship names.

**Primary recommendation:** Add fields to existing structs with `#[serde(default)]` for backward compatibility. Do NOT create new structs for relationships -- extend the existing `Join` struct to carry `from_alias` and `name` fields. Update `parse_args.rs` to extract PK columns and relationship metadata from the function-based DDL path. All existing tests must pass unchanged due to serde defaults.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MDL-01 | `TableRef` stores alias, physical table, and primary key columns | Add `pk_columns: Vec<String>` to `TableRef` with `#[serde(default)]` -- see Architecture Pattern 1 |
| MDL-02 | Relationships store from_alias, from_cols, to_alias (PK inferred from TABLES) | Add `from_alias` and `name` fields to `Join` struct with `#[serde(default)]` -- see Architecture Pattern 2 |
| MDL-03 | Dimensions and metrics store source table alias from qualified `alias.name` prefix | Already partially supported via `source_table: Option<String>`. Parse qualified names in DDL-06 path -- see Architecture Pattern 3 |
| MDL-04 | Composite primary keys supported (`PRIMARY KEY (col1, col2)`) | `Vec<String>` on `TableRef.pk_columns` handles multi-column PKs naturally -- see Architecture Pattern 1 |
| MDL-05 | Relationship names stored (informational, from `name AS ...` syntax) | Add `name: Option<String>` to `Join` struct with `#[serde(default)]` -- see Architecture Pattern 2 |
| DDL-06 | Function-based `create_semantic_view()` accepts equivalent PK/FK model parameters | Update `named_parameters()` to include PK columns in tables struct, add name to relationships struct -- see Architecture Pattern 4 |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde | 1.x | Struct serialization/deserialization | Already in Cargo.toml; `#[serde(default)]` is the backward compat mechanism |
| serde_json | 1.x | JSON round-trip for catalog persistence | Already in Cargo.toml; definitions stored as JSON in `semantic_layer._definitions` |
| libduckdb-sys | =1.4.4 | FFI types for DuckDB C API (duckdb_value, etc.) | Already pinned; used by `parse_args.rs` for struct field extraction |
| duckdb | =1.4.4 | VTab trait, LogicalTypeHandle, BindInfo | Already pinned; `named_parameters()` type signatures define DDL interface |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| proptest | 1.9 | Property-based tests for serde round-trips | Already in dev-dependencies; model round-trip tests |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Extending `Join` struct | New `Relationship` struct | New struct would break all existing code referencing `def.joins`; extending is safer and matches the incremental approach |
| `Vec<String>` for pk_columns | `HashSet<String>` | Vec preserves declaration order (important for composite PK column ordering in ON clause generation); HashSet does not |

## Architecture Patterns

### Recommended Changes to model.rs

The existing model.rs file structure remains the same. Only fields are added to existing structs:

```
src/model.rs         -- add fields to TableRef, Join
src/ddl/parse_args.rs -- update extraction logic for new struct fields
src/ddl/define.rs    -- update named_parameters() type signatures
```

### Pattern 1: Add pk_columns to TableRef (MDL-01, MDL-04)

**What:** Add a `pk_columns` field to store primary key column names per table.
**When to use:** Every table in a PK/FK semantic view declares its primary key.

Current `TableRef`:
```rust
pub struct TableRef {
    pub alias: String,
    pub table: String,
}
```

New `TableRef`:
```rust
pub struct TableRef {
    pub alias: String,
    pub table: String,
    /// Primary key column names for this table.
    /// Empty vec means no PK declared (backward compat with old stored JSON).
    #[serde(default)]
    pub pk_columns: Vec<String>,
}
```

**Key design decisions:**
- `Vec<String>` not `Option<Vec<String>>` -- empty vec is the natural serde default, and it avoids Option unwrapping in downstream code.
- `#[serde(default)]` ensures old stored JSON without `pk_columns` deserializes with empty vec.
- Composite PKs (MDL-04) work naturally -- `vec!["l_orderkey", "l_linenumber"]` for TPC-H lineitem.
- Order matters: PK column order determines ON clause column pairing in Phase 26.

### Pattern 2: Add from_alias and name to Join (MDL-02, MDL-05)

**What:** Add relationship source alias and optional name to the existing `Join` struct.
**When to use:** Every FK relationship needs to know which table it comes FROM (not just which table it goes TO).

Current `Join`:
```rust
pub struct Join {
    pub table: String,          // physical table name of the target
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on: String,             // legacy raw ON clause
    #[serde(default)]
    pub from_cols: Vec<String>, // legacy FK column names
    #[serde(default)]
    pub join_columns: Vec<JoinColumn>, // Phase 11.1 column pairs
}
```

New `Join`:
```rust
pub struct Join {
    pub table: String,          // physical table name of the target
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub on: String,             // legacy raw ON clause
    #[serde(default)]
    pub from_cols: Vec<String>, // legacy FK column names
    #[serde(default)]
    pub join_columns: Vec<JoinColumn>, // Phase 11.1 column pairs
    /// Source table alias for this relationship (e.g., "orders" in "orders(o_custkey) REFERENCES customer").
    /// Empty string for old stored JSON (serde default).
    #[serde(default)]
    pub from_alias: String,
    /// FK column names on the source table side.
    /// In PK/FK model, these are the columns that reference the target table's PK.
    /// Empty vec for old stored JSON (serde default).
    #[serde(default)]
    pub fk_columns: Vec<String>,
    /// Optional relationship name (e.g., "orders_to_customers" from `orders_to_customers AS ...` syntax).
    /// None for old stored JSON or unnamed relationships.
    #[serde(default)]
    pub name: Option<String>,
}
```

**Key design decisions:**
- `from_alias` is a `String` with `#[serde(default)]` (empty string) rather than `Option<String>`, because every new PK/FK relationship MUST have a source alias. Empty string = legacy/backward-compat.
- `fk_columns: Vec<String>` stores the FK column names on the source side. This is distinct from `join_columns` which stores column pairs (from, to). In the PK/FK model, the "to" columns are inferred from the target table's PK declaration, so we only need to store the FK column names.
- `name: Option<String>` because relationship names are truly optional (Snowflake allows unnamed relationships).
- The existing `join_columns` field remains for backward compat but is NOT used for new PK/FK definitions.

**Why not create a separate `Relationship` struct:**
The `Join` struct is referenced throughout the codebase: `expand.rs` (`resolve_joins`, `append_join_on_clause`), `ddl/define.rs`, `ddl/parse_args.rs`, `ddl/describe.rs`, `tests/expand_proptest.rs`. Creating a new `Relationship` struct would require either (a) a parallel field on `SemanticViewDefinition` (`relationships: Vec<Relationship>`) that duplicates `joins`, or (b) replacing `joins` everywhere. Both approaches are more disruptive than adding fields to `Join`. Phase 27 (cleanup) will remove legacy fields.

### Pattern 3: Source Table Alias on Dimensions/Metrics (MDL-03)

**What:** Parse qualified `alias.name` syntax to populate the `source_table` field on Dimension and Metric.
**When to use:** When the function-based DDL path receives dimension/metric names like `"orders.revenue"`.

The `source_table: Option<String>` field already exists on both `Dimension` and `Metric`. MDL-03 requires that when a dimension/metric is defined with a qualified name like `orders.revenue`, the `source_table` is set to `"orders"` and the `name` is set to `"revenue"`.

The function-based DDL path currently receives dimensions as `LIST(STRUCT(name VARCHAR, expr VARCHAR, source_table VARCHAR))`. Two approaches for qualified name support:

**Approach A (preferred): Parse qualified names in `parse_args.rs`.**
When `name` contains a dot and `source_table` is empty, split on the first dot: alias = prefix, bare_name = suffix. Set `source_table = Some(alias)` and `name = bare_name`.

```rust
// In parse_args.rs, after extracting dim_name and source_table_str:
let (final_name, source_table) = if source_table_str.is_empty() {
    if let Some(dot_pos) = dim_name.find('.') {
        let alias = dim_name[..dot_pos].to_string();
        let bare = dim_name[dot_pos + 1..].to_string();
        (bare, Some(alias))
    } else {
        (dim_name, None)
    }
} else {
    (dim_name, Some(source_table_str))
};
```

**Approach B: Require explicit `source_table` in function-based DDL.**
Leave name parsing unchanged; users must pass `source_table` explicitly. This is simpler but forces verbose function-call syntax.

Approach A is preferred because it aligns with the Snowflake syntax pattern (`alias.name`) and makes the function-based DDL path feel natural. The explicit `source_table` field remains as a fallback for when the qualified name doesn't apply.

### Pattern 4: Update Function-Based DDL Type Signatures (DDL-06)

**What:** Update `named_parameters()` in `define.rs` to include PK columns in the tables struct and relationship metadata in the relationships struct.
**When to use:** This is the DDL-06 requirement -- the function-based path must accept the same model as the SQL DDL path.

Current tables type: `LIST(STRUCT(alias VARCHAR, table VARCHAR))`
New tables type: `LIST(STRUCT(alias VARCHAR, table VARCHAR, pk_columns LIST(VARCHAR)))`

Current relationships type: `LIST(STRUCT(from_table VARCHAR, to_table VARCHAR, join_columns LIST(STRUCT(from VARCHAR, to VARCHAR))))`
New relationships type: `LIST(STRUCT(name VARCHAR, from_alias VARCHAR, to_alias VARCHAR, fk_columns LIST(VARCHAR)))`

**Key decisions:**
- The relationships struct changes from `(from_table, to_table, join_columns)` to `(name, from_alias, to_alias, fk_columns)`. This is a breaking change to the function-based DDL interface, which is acceptable because: (a) v0.5.2 is pre-release, (b) STATE.md explicitly states "NO backward compatibility needed", (c) the old syntax will be removed in Phase 27 (CLN-01).
- `fk_columns` is `LIST(VARCHAR)` not `LIST(STRUCT(from, to))` because in the PK/FK model, the "to" columns are inferred from the target table's PK declaration.
- `name` is VARCHAR (empty string = unnamed) rather than adding a separate optional field.

```rust
fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
    let varchar = || LogicalTypeHandle::from(LogicalTypeId::Varchar);

    let pk_columns_type = LogicalTypeHandle::list(&varchar());
    let tables_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
        ("alias", varchar()),
        ("table", varchar()),
        ("pk_columns", pk_columns_type),
    ]));

    let fk_columns_type = LogicalTypeHandle::list(&varchar());
    let relationships_type = LogicalTypeHandle::list(&LogicalTypeHandle::struct_type(&[
        ("name", varchar()),
        ("from_alias", varchar()),
        ("to_alias", varchar()),
        ("fk_columns", fk_columns_type),
    ]));

    // dimensions and metrics remain unchanged
    // ...
}
```

### Anti-Patterns to Avoid

- **Adding `deny_unknown_fields` to any struct:** This was explicitly removed in the past (see model.rs tests). Old stored JSON with extra fields must still deserialize. Adding it back would break catalog loading.
- **Using `Option<Vec<T>>` instead of `Vec<T>` with `#[serde(default)]`:** The empty-vec pattern is already established throughout the codebase. Mixing in `Option<Vec<T>>` adds unnecessary unwrapping and diverges from convention.
- **Creating a new `Relationship` struct separate from `Join`:** This duplicates the relationship concept and requires either a parallel field on `SemanticViewDefinition` or replacing `joins` everywhere. Extending `Join` is the incremental approach.
- **Changing existing field semantics:** For example, repurposing `join_columns` to store FK columns. The field has existing serialized data in catalogs. Add new fields instead.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON serialization backward compat | Manual JSON parsing with version checks | `#[serde(default)]` on new fields | Serde handles missing-field deserialization automatically; this pattern is already proven throughout model.rs |
| Qualified name parsing | Regex or full SQL parser | Simple `str::find('.')` split | Names are identifiers (no nested dots in table aliases); a single dot split is correct and matches expand.rs behavior |
| DuckDB struct type declaration | Manual C API type building | `LogicalTypeHandle::struct_type(&[...])` | The duckdb-rs API handles all the FFI complexity; existing code in define.rs shows the pattern |

## Common Pitfalls

### Pitfall 1: Breaking Serde Backward Compatibility
**What goes wrong:** Adding a new required field (without `#[serde(default)]`) causes old stored JSON to fail deserialization, silently corrupting the catalog.
**Why it happens:** The catalog table `semantic_layer._definitions` contains JSON written by older versions of the extension. If a new field is required, `serde_json::from_str` returns an error.
**How to avoid:** Every new field MUST use `#[serde(default)]` or `#[serde(default, skip_serializing_if = "...")]`. The existing test `unknown_fields_are_allowed()` validates this. Add a specific test for each new field showing old JSON without the field still deserializes.
**Warning signs:** `cargo test` failures in `model::tests` module.

### Pitfall 2: Field Order Mismatch in DuckDB Struct Types
**What goes wrong:** The struct child index in `parse_args.rs` (`extract_struct_child_varchar(child, 0)`, `...(child, 1)`, etc.) doesn't match the field order in `named_parameters()`.
**Why it happens:** DuckDB struct children are accessed by positional index, not by name. If `named_parameters()` declares `("alias", "table", "pk_columns")` but extraction code reads index 0 as `table` and index 1 as `alias`, values are silently swapped.
**How to avoid:** Keep extraction order in `parse_args.rs` exactly matching declaration order in `define.rs::named_parameters()`. Add comments documenting the index-to-field mapping. Test round-trip through actual DuckDB function calls.
**Warning signs:** Wrong values in parsed definitions; tests pass for wrong reasons because both name and expr are strings.

### Pitfall 3: Empty String vs None Semantics
**What goes wrong:** Code checks `source_table.is_some()` but the value is `Some("")` (empty string), leading to incorrect join resolution or qualified name matching.
**Why it happens:** `extract_varchar` returns empty string for null DuckDB values. If the code wraps this in `Some("")` instead of checking for emptiness first, downstream logic gets confused.
**How to avoid:** Always filter empty strings to `None` when populating `Option<String>` fields. The existing pattern in `parse_args.rs` already does this correctly: `if source_table_str.is_empty() { None } else { Some(source_table_str) }`. Apply the same pattern to new fields.
**Warning signs:** Unexpected join inclusion; wrong source table resolution.

### Pitfall 4: Forgetting to Update the Arbitrary Derive
**What goes wrong:** `#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]` is on all model structs. If a new field's type doesn't implement `Arbitrary`, the build breaks under the `arbitrary` feature (used by fuzz targets).
**Why it happens:** `Vec<String>`, `Option<String>`, and `String` all implement `Arbitrary`, so this is unlikely for this phase. But adding a new struct type (e.g., `HashMap`) would break it.
**How to avoid:** All new fields use types that already implement `Arbitrary` (`String`, `Vec<String>`, `Option<String>`). If a new type is needed, check that it implements `Arbitrary` or add a manual implementation.
**Warning signs:** `cargo build --features arbitrary` fails.

### Pitfall 5: Updating parse_args Without Updating Existing Tests
**What goes wrong:** The function-based DDL tests in sqllogictest or Python integration tests hardcode the old struct field order. After changing `named_parameters()`, these tests pass `from_table` where the new code expects `name`, causing silent data corruption.
**Why it happens:** The DDL type signature change is a breaking change to the function call interface.
**How to avoid:** Update all sqllogictest files and Python test files that use `create_semantic_view()` with the new parameter format. Search for `create_semantic_view` across all `.test`, `.py`, and `.rs` files.
**Warning signs:** SQL logic tests fail with type mismatch or wrong column values.

## Code Examples

### Example 1: New TableRef with pk_columns (serde round-trip)
```rust
// Source: Derived from existing model.rs patterns
let tr = TableRef {
    alias: "lineitem".to_string(),
    table: "tpch.lineitem".to_string(),
    pk_columns: vec!["l_orderkey".to_string(), "l_linenumber".to_string()],
};
let json = serde_json::to_string(&tr).unwrap();
// {"alias":"lineitem","table":"tpch.lineitem","pk_columns":["l_orderkey","l_linenumber"]}
let rt: TableRef = serde_json::from_str(&json).unwrap();
assert_eq!(rt.pk_columns, vec!["l_orderkey", "l_linenumber"]);
```

### Example 2: New Join with from_alias, fk_columns, and name
```rust
// Source: Derived from existing model.rs patterns
let join = Join {
    table: "customers".to_string(),     // physical target table
    on: String::new(),                   // empty (PK/FK model, not legacy)
    from_cols: vec![],                   // empty (legacy field)
    join_columns: vec![],               // empty (Phase 11.1 legacy)
    from_alias: "orders".to_string(),   // source table alias
    fk_columns: vec!["o_custkey".to_string()],  // FK columns on source
    name: Some("orders_to_customers".to_string()), // optional rel name
};
let json = serde_json::to_string(&join).unwrap();
let rt: Join = serde_json::from_str(&json).unwrap();
assert_eq!(rt.from_alias, "orders");
assert_eq!(rt.fk_columns, vec!["o_custkey"]);
assert_eq!(rt.name.as_deref(), Some("orders_to_customers"));
```

### Example 3: Old JSON backward compat (no new fields)
```rust
// Source: Existing model.rs test pattern
let json = r#"{"table":"customers","on":"a.id=b.id"}"#;
let join: Join = serde_json::from_str(json).unwrap();
assert_eq!(join.from_alias, "");       // serde default
assert!(join.fk_columns.is_empty());   // serde default
assert!(join.name.is_none());          // serde default
```

### Example 4: Qualified name parsing for dimensions
```rust
// Source: Derived from expand.rs find_dimension pattern
let dim_name = "orders.revenue";
if let Some(dot_pos) = dim_name.find('.') {
    let alias = &dim_name[..dot_pos];    // "orders"
    let bare_name = &dim_name[dot_pos + 1..]; // "revenue"
    // Set source_table = Some("orders"), name = "revenue"
}
```

### Example 5: Complete PK/FK definition (target state)
```rust
// Source: Derived from Snowflake TPC-H example + existing model patterns
let def = SemanticViewDefinition {
    base_table: "tpch.orders".to_string(),
    tables: vec![
        TableRef {
            alias: "orders".to_string(),
            table: "tpch.orders".to_string(),
            pk_columns: vec!["o_orderkey".to_string()],
        },
        TableRef {
            alias: "customer".to_string(),
            table: "tpch.customer".to_string(),
            pk_columns: vec!["c_custkey".to_string()],
        },
        TableRef {
            alias: "lineitem".to_string(),
            table: "tpch.lineitem".to_string(),
            pk_columns: vec!["l_orderkey".to_string(), "l_linenumber".to_string()],
        },
    ],
    dimensions: vec![
        Dimension {
            name: "order_date".to_string(),
            expr: "orders.o_orderdate".to_string(),
            source_table: Some("orders".to_string()),
            output_type: None,
        },
        Dimension {
            name: "customer_name".to_string(),
            expr: "customer.c_name".to_string(),
            source_table: Some("customer".to_string()),
            output_type: None,
        },
    ],
    metrics: vec![
        Metric {
            name: "order_count".to_string(),
            expr: "count(orders.o_orderkey)".to_string(),
            source_table: Some("orders".to_string()),
            output_type: None,
        },
    ],
    filters: vec![],
    joins: vec![
        Join {
            table: "tpch.customer".to_string(),
            on: String::new(),
            from_cols: vec![],
            join_columns: vec![],
            from_alias: "orders".to_string(),
            fk_columns: vec!["o_custkey".to_string()],
            name: None,
        },
        Join {
            table: "tpch.lineitem".to_string(),
            on: String::new(),
            from_cols: vec![],
            join_columns: vec![],
            from_alias: "orders".to_string(),
            fk_columns: vec!["l_orderkey".to_string()],
            name: Some("lineitem_to_orders".to_string()),
        },
    ],
    facts: vec![],
    column_type_names: vec![],
    column_types_inferred: vec![],
};
let json = serde_json::to_string(&def).unwrap();
let rt: SemanticViewDefinition = serde_json::from_str(&json).unwrap();
assert_eq!(rt.tables[2].pk_columns.len(), 2); // composite PK
assert_eq!(rt.joins[0].from_alias, "orders");
assert_eq!(rt.joins[0].fk_columns, vec!["o_custkey"]);
```

### Example 6: Function-based DDL call (target syntax for DDL-06)
```sql
-- New function-based DDL syntax with PK/FK model
FROM create_semantic_view(
    'tpch_orders',
    tables := [
        {'alias': 'orders', 'table': 'tpch.orders', 'pk_columns': ['o_orderkey']},
        {'alias': 'customer', 'table': 'tpch.customer', 'pk_columns': ['c_custkey']},
    ],
    relationships := [
        {'name': '', 'from_alias': 'orders', 'to_alias': 'customer', 'fk_columns': ['o_custkey']},
    ],
    dimensions := [
        {'name': 'order_date', 'expr': 'orders.o_orderdate', 'source_table': 'orders'},
    ],
    metrics := [
        {'name': 'order_count', 'expr': 'count(o_orderkey)', 'source_table': 'orders'},
    ]
);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Raw ON clause strings (`join.on`) | Column-pair structs (`join.join_columns`) | Phase 11.1 (v0.2.0) | ON clauses generated from structured data |
| No table aliases | `TableRef` with alias + table | Phase 11.1 (v0.2.0) | Aliases used in generated SQL |
| No PK declarations | (this phase) PK columns on `TableRef` | Phase 24 (v0.5.2) | Enables PK inference in relationships |
| Explicit column pairs in relationships | (this phase) FK-only columns with PK inferred | Phase 24 (v0.5.2) | Simpler DDL; PK declared once per table |

**Deprecated/outdated:**
- `join.on` (raw ON clause string): Legacy field from Phase 10 and earlier. Not written by new DDL paths. Kept for backward compat. Will be removed in Phase 27 (CLN-03).
- `join.from_cols`: Legacy field from Phase 11 (pre-11.1). Not written by current DDL paths. Kept for backward compat. Will be removed in Phase 27.
- `join.join_columns` (column pairs): Phase 11.1 format. Superseded by PK/FK model where PK is declared on the table and FK columns are on the relationship. Kept for backward compat. Will be removed in Phase 27.

## Snowflake Reference Design

The Snowflake `CREATE SEMANTIC VIEW` syntax is the design reference for this project (per STATE.md decisions). Key observations from the Snowflake docs that inform Phase 24:

1. **Tables declare PK inline:** `orders AS schema.orders PRIMARY KEY (o_orderkey)`. PK is per-table, not per-relationship.
2. **Relationships reference aliases, not physical tables:** `orders (o_custkey) REFERENCES customer`. The alias is the stable identifier; physical table names are resolved via the TABLES clause.
3. **FK columns are on the source side only:** `orders (o_custkey) REFERENCES customer` -- the FK columns (`o_custkey`) belong to `orders`. The target PK columns (`c_custkey`) are inferred from `customer`'s PRIMARY KEY declaration.
4. **Relationship names are optional:** Both `rel_name AS orders (fk) REFERENCES target` and `orders (fk) REFERENCES target` are valid.
5. **Dimensions use qualified names:** `orders.order_date AS o_orderdate` -- the table alias prefix is part of the dimension identity, not just the expression.
6. **Composite PKs are supported:** `lineitem PRIMARY KEY (l_orderkey, l_linenumber)`.

## Open Questions

1. **Should `Join.table` store alias or physical table name?**
   - What we know: Currently `Join.table` stores the physical table name (resolved from alias in `parse_args.rs` line 122-126). The Snowflake model uses aliases exclusively. The expansion engine (`expand.rs`) uses `Join.table` to match against physical table names.
   - What's unclear: Switching to alias-based storage would be cleaner for the PK/FK model but would require updating `resolve_joins()` and `append_join_on_clause()`.
   - Recommendation: Keep `Join.table` as physical table name for Phase 24 (backward compat). In Phase 27, when the expansion engine is rewritten for alias-based expansion, `Join.table` can be deprecated in favor of alias-only resolution. For now, store the target alias in a comment or use `to_alias` derivation via table lookup at expansion time.

2. **Should we store `to_alias` explicitly or derive it from the tables list?**
   - What we know: The Snowflake model uses `REFERENCES to_alias`. In the current model, `Join.table` is the physical table name, and the alias is looked up from `def.tables` at expansion time.
   - Recommendation: Do NOT add a `to_alias` field. The alias can always be derived from `def.tables` by matching `Join.table` against `TableRef.table`. This avoids data duplication and keeps the model DRY. Phase 26 (join resolution) will handle the alias lookup.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (proptest 1.9) + sqllogictest + Python integration |
| Config file | Cargo.toml, justfile |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MDL-01 | TableRef stores pk_columns | unit | `cargo test model::tests::phase24 -x` | Wave 0 |
| MDL-02 | Join stores from_alias, fk_columns | unit | `cargo test model::tests::phase24 -x` | Wave 0 |
| MDL-03 | Dim/Metric source_table from qualified name | unit | `cargo test model::tests::phase24 -x` | Wave 0 |
| MDL-04 | Composite PK round-trip | unit | `cargo test model::tests::phase24 -x` | Wave 0 |
| MDL-05 | Relationship name stored | unit | `cargo test model::tests::phase24 -x` | Wave 0 |
| DDL-06 | Function DDL accepts PK/FK params | unit | `cargo test ddl::parse_args -x` (for parse logic) | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `src/model.rs` -- add `phase24_model_tests` test module with serde round-trip tests for new fields
- [ ] `src/model.rs` -- add backward compat tests (old JSON without new fields deserializes correctly)
- [ ] `src/ddl/parse_args.rs` -- add unit tests for qualified name parsing and new struct field extraction

## Sources

### Primary (HIGH confidence)
- Project source code: `src/model.rs`, `src/expand.rs`, `src/ddl/define.rs`, `src/ddl/parse_args.rs` -- read directly, all patterns verified
- [Snowflake CREATE SEMANTIC VIEW docs](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- PK/FK syntax reference
- [Snowflake semantic view example](https://docs.snowflake.com/en/user-guide/views-semantic/example) -- TPC-H worked example

### Secondary (MEDIUM confidence)
- serde documentation for `#[serde(default)]` behavior -- well-established pattern, already used throughout project

### Tertiary (LOW confidence)
- None. All findings verified against source code and official documentation.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all existing libraries
- Architecture: HIGH - extending existing model patterns with proven serde defaults
- Pitfalls: HIGH - all identified from existing codebase patterns and known failure modes

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable domain, no external dependencies changing)
