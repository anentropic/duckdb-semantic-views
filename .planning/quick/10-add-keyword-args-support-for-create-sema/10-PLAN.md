---
phase: 10-add-keyword-args-support-for-create-sema
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - src/ddl/define.rs
  - src/ddl/drop.rs
  - src/ddl/parse_args.rs
  - src/lib.rs
  - test/sql/phase2_ddl.test
autonomous: true
requirements: [KWARG-01]

must_haves:
  truths:
    - "create_semantic_view works with keyword args syntax (name :=, tables :=, etc.)"
    - "create_semantic_view still works with positional args (backward compat)"
    - "create_or_replace_semantic_view and create_semantic_view_if_not_exists support keyword args"
    - "drop_semantic_view and drop_semantic_view_if_exists support keyword args"
    - "All 5 DDL functions return a single row with the view name (VARCHAR)"
    - "Full test suite passes (cargo test, sqllogictest, DuckLake CI)"
  artifacts:
    - path: "src/ddl/define.rs"
      provides: "DefineSemanticViewVTab implementing VTab with named_parameters()"
      contains: "impl VTab for DefineSemanticViewVTab"
    - path: "src/ddl/drop.rs"
      provides: "DropSemanticViewVTab implementing VTab with named_parameters()"
      contains: "impl VTab for DropSemanticViewVTab"
    - path: "src/ddl/parse_args.rs"
      provides: "parse_define_args_from_bind() that reads from BindInfo named params"
      contains: "parse_define_args_from_bind"
    - path: "src/lib.rs"
      provides: "Registration changed from register_scalar_function_with_state to register_table_function_with_extra_info"
      contains: "register_table_function_with_extra_info"
    - path: "test/sql/phase2_ddl.test"
      provides: "SQL tests exercising keyword args syntax for create and drop"
      contains: "tables :="
  key_links:
    - from: "src/lib.rs"
      to: "src/ddl/define.rs"
      via: "register_table_function_with_extra_info::<DefineSemanticViewVTab, _>"
      pattern: "register_table_function_with_extra_info::<DefineSemanticViewVTab"
    - from: "src/lib.rs"
      to: "src/ddl/drop.rs"
      via: "register_table_function_with_extra_info::<DropSemanticViewVTab, _>"
      pattern: "register_table_function_with_extra_info::<DropSemanticViewVTab"
    - from: "src/ddl/define.rs"
      to: "src/ddl/parse_args.rs"
      via: "parse_define_args_from_bind() called in VTab::bind()"
      pattern: "parse_define_args_from_bind"
---

<objective>
Convert all 5 DDL functions (create_semantic_view, create_or_replace_semantic_view, create_semantic_view_if_not_exists, drop_semantic_view, drop_semantic_view_if_exists) from VScalar to VTab table functions so they support DuckDB's `param := value` named parameter syntax.

Purpose: VScalar only supports `ScalarFunctionSignature::exact` (positional params). Named parameters (`param := value`) are only available via `VTab::named_parameters()`. Converting to VTab enables the ergonomic `create_semantic_view(name := 'my_view', tables := [...], ...)` syntax while preserving full backward compatibility with positional args.

Output: All DDL functions registered as table functions with named_parameters(), existing positional syntax still works, new keyword args syntax tested via sqllogictest.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src/ddl/define.rs
@src/ddl/drop.rs
@src/ddl/parse_args.rs
@src/ddl/describe.rs
@src/ddl/list.rs
@src/lib.rs
@test/sql/phase2_ddl.test

<interfaces>
<!-- Existing VTab pattern used by describe/list/semantic_view — the DDL functions must follow this same pattern -->

From src/ddl/describe.rs (VTab pattern with extra_info):
```rust
use duckdb::vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab};

impl VTab for DescribeSemanticViewVTab {
    type BindData = DescribeBindData;
    type InitData = DescribeInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
        let name = bind.get_parameter(0).to_string();
        let state_ptr = bind.get_extra_info::<CatalogState>();
        // ...
    }
    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![LogicalTypeHandle::from(LogicalTypeId::Varchar)])
    }
}
```

From src/query/table_function.rs (VTab with named_parameters):
```rust
fn named_parameters() -> Option<Vec<(String, LogicalTypeHandle)>> {
    Some(vec![
        ("dimensions".to_string(), LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar))),
        ("metrics".to_string(), LogicalTypeHandle::list(&LogicalTypeHandle::from(LogicalTypeId::Varchar))),
    ])
}
```

From src/query/table_function.rs (extracting named params in bind):
```rust
let dimensions = match bind.get_named_parameter("dimensions") {
    Some(ref val) => unsafe { extract_list_strings(val) },
    None => vec![],
};
```

Registration pattern from src/lib.rs:
```rust
con.register_table_function_with_extra_info::<DescribeSemanticViewVTab, _>(
    "describe_semantic_view",
    &catalog_state,
)?;
```

From src/ddl/define.rs (current DefineState — needs to become extra_info):
```rust
pub struct DefineState {
    pub catalog: CatalogState,
    pub persist_conn: Option<ffi::duckdb_connection>,
    pub or_replace: bool,
    pub if_not_exists: bool,
}
```

From src/ddl/parse_args.rs (current parsing from DataChunkHandle — needs BindInfo variant):
```rust
pub struct ParsedDefineArgs {
    pub name: String,
    pub def: SemanticViewDefinition,
}
```

From src/query/table_function.rs (extracting struct fields from Value via FFI):
```rust
unsafe fn value_raw_ptr(value: &Value) -> ffi::duckdb_value {
    let ptr_to_value = std::ptr::from_ref(value).cast::<ffi::duckdb_value>();
    std::ptr::read(ptr_to_value)
}

pub(crate) unsafe fn extract_list_strings(value: &Value) -> Vec<String> {
    let value_ptr = value_raw_ptr(value);
    let size = ffi::duckdb_get_list_size(value_ptr);
    // ...
}
```
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Convert define.rs and drop.rs from VScalar to VTab, add parse_args_from_bind, update registration</name>
  <files>src/ddl/define.rs, src/ddl/drop.rs, src/ddl/parse_args.rs, src/lib.rs</files>
  <action>
This task converts all 5 DDL functions from VScalar to VTab table functions.

**src/ddl/parse_args.rs** -- Add a new function `parse_define_args_from_bind`:

1. Add a new public function `parse_define_args_from_bind(bind: &BindInfo) -> Result<ParsedDefineArgs, String>` that reads all 6 params from `BindInfo` using named params, falling back to positional.
2. The function must extract each of the 6 LIST(STRUCT) values from `bind.get_named_parameter("tables")` etc., OR from `bind.get_parameter(N)` if named params are absent. Since DuckDB table functions with both `parameters()` and `named_parameters()` pass positional args via `get_parameter(N)` and named args via `get_named_parameter("name")`, the bind function should: read the view name from `bind.get_parameter(0)` (always positional), then for each of tables/relationships/dimensions/time_dimensions/metrics, try `bind.get_named_parameter("tables")` first, fall back to `bind.get_parameter(1)` etc.
3. To extract LIST(STRUCT(...)) values from a `duckdb::vtab::Value`, use the FFI approach from `src/query/table_function.rs`: call `value_raw_ptr()` to get the `duckdb_value`, then `duckdb_get_list_size()` + `duckdb_get_list_child()` to iterate list elements, then for each child struct use `duckdb_get_map_size`/`duckdb_struct_type_child_count`/`duckdb_get_struct_child` or equivalently `duckdb_struct_extract_entry` to extract struct fields. Specifically: for each LIST element (a STRUCT), use `ffi::duckdb_struct_extract_entry(child_val, field_name_cstr)` to get each field, then `ffi::duckdb_get_varchar()` to read it as a string. Remember to call `duckdb_destroy_value` on extracted values and `duckdb_free` on varchar char pointers.
4. Keep the existing `parse_define_args(DataChunkHandle)` function -- it can be removed later but keeping it avoids breaking anything during transition. Mark it `#[allow(dead_code)]` or simply let the compiler warn. Actually, since it will no longer be called, remove it and the `read_str` helper. Keep `validate_granularity` and `ParsedDefineArgs` and the unit tests for validate_granularity.
5. Add necessary imports: `use duckdb::vtab::{BindInfo, Value};` and `use libduckdb_sys as ffi;` and `use std::ffi::{CStr, CString};` and `use std::os::raw::c_void;`.
6. The `value_raw_ptr` and `extract_list_strings` functions from `src/query/table_function.rs` should be made `pub(crate)` (they already are). Reuse `value_raw_ptr` from table_function -- either import it or duplicate the 3-line helper locally. Prefer importing: `use crate::query::table_function::value_raw_ptr;`. Note: `value_raw_ptr` is currently `unsafe fn` -- the call site in parse_args must be unsafe too.
7. IMPORTANT: The LIST(STRUCT) named parameter values are DuckDB `duckdb_value` handles representing complex nested types. To extract struct fields from a list of structs: iterate list children with `duckdb_get_list_child(list_val, i)`, then for each struct child, use `duckdb_struct_extract_entry(child, field_cstr)` where `field_cstr` is a C string like `c"alias"`. Each extracted value must be read with `duckdb_get_varchar()` and then freed with `duckdb_free` + `duckdb_destroy_value`.

**src/ddl/define.rs** -- Convert DefineSemanticView from VScalar to VTab:

1. Rename `DefineSemanticView` to `DefineSemanticViewVTab`.
2. Change `impl VScalar for DefineSemanticView` to `impl VTab for DefineSemanticViewVTab`.
3. Create `DefineBindData` struct with fields: `name: String` (the view name returned as result). The actual DDL work (catalog insert, persist, type inference) happens in `bind()`, not `func()`. This is the correct pattern because VTab bind runs once, similar to how the current VScalar invoke runs once per row. Since DDL is a side-effect operation, doing it in bind is correct (same as how describe_semantic_view reads catalog in bind).
4. Create `DefineInitData` with `done: AtomicBool` (same pattern as describe.rs).
5. Implement `fn bind(bind: &BindInfo)`:
   - Call `parse_define_args_from_bind(bind)` to get ParsedDefineArgs
   - Get `DefineState` from `bind.get_extra_info::<DefineState>()`
   - Do the DDL-time type inference (same as current invoke code, using `state.persist_conn`)
   - Serialize to JSON, persist (write-first), then update in-memory catalog (same logic as current invoke)
   - Add result column: `bind.add_result_column("view_name", VARCHAR)`
   - Return `DefineBindData { name }`
6. Implement `fn func()`: emit single row with view name, mark done. Same pattern as describe.rs.
7. Implement `fn parameters()`: return `Some(vec![VARCHAR])` -- the view name is positional param 0.
8. Implement `fn named_parameters()`: return the 5 named params:
   ```rust
   Some(vec![
       ("tables".to_string(), tables_type),
       ("relationships".to_string(), relationships_type),
       ("dimensions".to_string(), dimensions_type),
       ("time_dimensions".to_string(), time_dimensions_type),
       ("metrics".to_string(), metrics_type),
   ])
   ```
   where each type is the same LIST(STRUCT(...)) type currently built in `signatures()`. Extract the type-building code into a helper or inline it.
9. Remove the VScalar import and the `signatures()` / `invoke()` methods.
10. IMPORTANT: The DDL side-effects (catalog_insert, persist_define) MUST happen in `bind()`, not in `func()`. `func()` only emits the result row. This is because `bind()` is called once during query planning, and for DDL-style table functions this is the correct place for the side effect. The `func()` call happens during execution and just returns the result.

**src/ddl/drop.rs** -- Convert DropSemanticView from VScalar to VTab:

1. Rename `DropSemanticView` to `DropSemanticViewVTab`.
2. Same VTab conversion pattern. `bind()` does the catalog delete + persist. `func()` emits one row.
3. `parameters()`: `Some(vec![VARCHAR])` -- view name positional.
4. `named_parameters()`: `Some(vec![("name".to_string(), VARCHAR)])` -- also accept `name :=` as keyword. Actually, since there is only 1 param (the view name) and it is always positional, `named_parameters()` can return `None`. The view name will always be the first positional param. Keep it simple.
5. Read view name from `bind.get_parameter(0).to_string()`.
6. Get `DropState` from `bind.get_extra_info::<DropState>()`.

**src/lib.rs** -- Update registration:

1. Change imports: `DefineSemanticView` -> `DefineSemanticViewVTab`, `DropSemanticView` -> `DropSemanticViewVTab`.
2. Replace all 3 `register_scalar_function_with_state::<DefineSemanticView>` calls with `register_table_function_with_extra_info::<DefineSemanticViewVTab, _>`.
3. Replace both `register_scalar_function_with_state::<DropSemanticView>` calls with `register_table_function_with_extra_info::<DropSemanticViewVTab, _>`.
4. The state passed as extra_info remains the same (`DefineState` / `DropState`), accessed via `get_extra_info` in bind.

**Key differences from VScalar -> VTab:**
- VScalar: `State` type is set at registration, `invoke(state, input_chunk, output_vector)` processes rows
- VTab: extra_info is set at registration, `bind(bind_info) -> BindData` does work + declares output schema, `func(func_info, output_chunk)` emits rows
- VScalar output is a single vector (one column); VTab output is a DataChunkHandle (multi-column). For DDL returning just one VARCHAR column, use `output.flat_vector(0).insert(0, name)` + `output.set_len(1)`.

**Syntax change for callers:**
- Old (VScalar): `SELECT create_semantic_view('name', [...], ...);`
- New positional (VTab): `SELECT * FROM create_semantic_view('name', [...], ...);` OR `FROM create_semantic_view(...)` -- table functions need FROM, not bare SELECT
- New keyword (VTab): `FROM create_semantic_view('name', tables := [...], dimensions := [...], ...)`

Note: Converting from scalar to table function changes the SQL calling convention. Users must now use `FROM create_semantic_view(...)` or `SELECT * FROM create_semantic_view(...)` instead of `SELECT create_semantic_view(...)`. This is a breaking change but acceptable since we are pre-1.0 and the keyword args benefit outweighs the cost.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && cargo build --features extension 2>&1 | tail -20</automated>
  </verify>
  <done>All 5 DDL functions compile as VTab implementations. DefineSemanticViewVTab has named_parameters() returning the 5 LIST(STRUCT) param types. DropSemanticViewVTab compiles with VTab. lib.rs registers all via register_table_function_with_extra_info. No VScalar references remain for DDL functions.</done>
</task>

<task type="auto">
  <name>Task 2: Update SQL tests for table function syntax and add keyword args test cases</name>
  <files>test/sql/phase2_ddl.test, test/sql/phase4_query.test, test/sql/phase2_restart.test, test/sql/semantic_views.test</files>
  <action>
Update all SQL test files that call DDL functions to use table function syntax, and add new test cases for keyword args.

**test/sql/phase2_ddl.test** -- Major updates:

1. Change ALL `SELECT create_semantic_view(...)` to `SELECT * FROM create_semantic_view(...)` (or just `FROM ...`). Table functions require FROM syntax.
2. Change ALL `SELECT create_or_replace_semantic_view(...)` to `SELECT * FROM create_or_replace_semantic_view(...)`.
3. Change ALL `SELECT create_semantic_view_if_not_exists(...)` to `SELECT * FROM create_semantic_view_if_not_exists(...)`.
4. Change ALL `SELECT drop_semantic_view(...)` to `SELECT * FROM drop_semantic_view(...)`.
5. Change ALL `SELECT drop_semantic_view_if_exists(...)` to `SELECT * FROM drop_semantic_view_if_exists(...)`.
6. Add a NEW test section (section 16) that creates a semantic view using keyword args syntax:
```sql
# ============================================================
# 16. Keyword args syntax — create_semantic_view with named params
# ============================================================

statement ok
SELECT * FROM create_semantic_view(
    'kwarg_test',
    tables := [{'alias': 'o', 'table': 'orders'}],
    relationships := [],
    dimensions := [{'name': 'region', 'expr': 'region', 'source_table': 'o'}],
    time_dimensions := [],
    metrics := [{'name': 'revenue', 'expr': 'sum(amount)', 'source_table': 'o'}]
);

# Verify it was created
query TT
SELECT name, base_table FROM describe_semantic_view('kwarg_test');
----
kwarg_test	orders

# Clean up
statement ok
SELECT * FROM drop_semantic_view('kwarg_test');
```

7. Add another test that mixes positional name with keyword args for the remaining params:
```sql
# 17. Mixed positional + keyword args
statement ok
SELECT * FROM create_semantic_view(
    'mixed_test',
    tables := [{'alias': 'o', 'table': 'orders'}],
    dimensions := [{'name': 'status', 'expr': 'status', 'source_table': 'o'}],
    metrics := [{'name': 'total', 'expr': 'count(*)', 'source_table': 'o'}]
);

query TT
SELECT name, base_table FROM describe_semantic_view('mixed_test');
----
mixed_test	orders

statement ok
SELECT * FROM drop_semantic_view('mixed_test');
```
Note: When named params are used, omitting optional ones (relationships, time_dimensions) should default to empty lists.

8. Add a test for drop with keyword args (if drop supports named params). Since drop only takes 1 positional VARCHAR, this test just confirms `FROM drop_semantic_view('name')` works.

**test/sql/phase4_query.test** -- Update DDL calls:
Change any `SELECT create_semantic_view(...)` to `SELECT * FROM create_semantic_view(...)` and any `SELECT drop_semantic_view(...)` to `SELECT * FROM drop_semantic_view(...)`.

**test/sql/phase2_restart.test** -- Update DDL calls:
Same pattern: add `* FROM` to all DDL function calls.

**test/sql/semantic_views.test** -- Update DDL calls:
Same pattern: add `* FROM` to all DDL function calls.

IMPORTANT: After updating tests, verify that `statement ok` assertions still match — table functions return a result row (the view name), not a scalar. The `statement ok` directive should still work since it only checks success/failure, not the output.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && just test-all 2>&1 | tail -30</automated>
  </verify>
  <done>All SQL test files use `FROM` syntax for DDL calls. New test sections validate keyword args syntax for create_semantic_view. Full test suite passes: cargo test, sqllogictest, DuckLake CI.</done>
</task>

</tasks>

<verification>
Full test suite must pass:
```bash
just test-all
```
This runs cargo test (Rust unit + proptest), sqllogictest (integration), and DuckLake CI tests.

Additionally verify:
1. `just build` succeeds (extension binary compiles)
2. Keyword args syntax works: `FROM create_semantic_view('test', tables := [...], dimensions := [...], metrics := [...])`
3. Positional syntax still works: `FROM create_semantic_view('test', [...], [], [...], [], [...])`
4. All existing test files pass without modification beyond the SELECT->FROM syntax change
</verification>

<success_criteria>
- All 5 DDL functions registered as VTab table functions (not VScalar)
- named_parameters() returns correct LIST(STRUCT) types for the 5 define params
- Keyword args syntax tested in sqllogictest
- Positional args backward compatibility preserved (existing tests pass with FROM syntax)
- `just test-all` passes completely
</success_criteria>

<output>
After completion, create `.planning/quick/10-add-keyword-args-support-for-create-sema/10-SUMMARY.md`
</output>
