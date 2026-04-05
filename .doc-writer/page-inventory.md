# Page Inventory -- v0.5.5 SHOW/DESCRIBE Schema Updates

**Generated:** 2026-04-02
**Type:** Content update pass (6 existing reference pages with outdated column schemas)

## Pages Requiring Updates

These 6 existing reference pages have outdated output schemas from the v0.5.5 SHOW/DESCRIBE alignment work. Each page needs its Output Columns section rewritten and all example output tables updated to match the new column schemas verified against source code.

| # | Type | Title | Key Sections to Update | File Path |
|---|------|-------|------------------------|-----------|
| 1 | reference | SHOW SEMANTIC VIEWS | Output Columns, all Examples | `docs/reference/show-semantic-views.rst` |
| 2 | reference | SHOW SEMANTIC DIMENSIONS | Output Columns, note about data_type, all Examples | `docs/reference/show-semantic-dimensions.rst` |
| 3 | reference | SHOW SEMANTIC METRICS | Output Columns, derived metrics note, all Examples | `docs/reference/show-semantic-metrics.rst` |
| 4 | reference | SHOW SEMANTIC FACTS | Output Columns, remove "no data_type" note, all Examples | `docs/reference/show-semantic-facts.rst` |
| 5 | reference | SHOW SEMANTIC DIMENSIONS FOR METRIC | Output Columns, "same schema" cross-ref, Fan Trap Filtering examples, all Examples | `docs/reference/show-semantic-dimensions-for-metric.rst` |
| 6 | reference | DESCRIBE SEMANTIC VIEW | Meta description, page intro, Output Columns (complete replacement), JSON tip, all Examples | `docs/reference/describe-semantic-view.rst` |

## Detailed Change Specifications

### Page 1: SHOW SEMANTIC VIEWS (`docs/reference/show-semantic-views.rst`)

**Schema change:** 2 columns -> 5 columns.

Old columns: `name` (VARCHAR), `base_table` (VARCHAR)

New columns (verified from `src/ddl/list.rs` lines 56-68):

| Column | Type | Description |
|--------|------|-------------|
| `created_on` | VARCHAR | Timestamp when the semantic view was created. |
| `name` | VARCHAR | The semantic view name. |
| `kind` | VARCHAR | Always `SEMANTIC_VIEW`. |
| `database_name` | VARCHAR | The DuckDB database containing the view (e.g., `memory`). |
| `schema_name` | VARCHAR | The DuckDB schema containing the view (e.g., `main`). |

**Sections to update:**
- **Output Columns:** Replace "2 columns" table with 5-column table above.
- **All example output tables:** Change from 2-column (`name | base_table`) to 5-column format. The `created_on` column is a non-deterministic timestamp, so examples should either show a realistic timestamp placeholder or demonstrate with `SELECT name, kind, database_name, schema_name FROM (SHOW SEMANTIC VIEWS);` to skip it.
- **Prose:** Any reference to `base_table` column must be removed.

---

### Page 2: SHOW SEMANTIC DIMENSIONS (`docs/reference/show-semantic-dimensions.rst`)

**Schema change:** 5 columns -> 6 columns.

Old columns: `semantic_view_name`, `name`, `expr`, `source_table`, `data_type`

New columns (verified from `src/ddl/show_dims.rs` lines 44-62):

| Column | Type | Description |
|--------|------|-------------|
| `database_name` | VARCHAR | The DuckDB database containing the semantic view. |
| `schema_name` | VARCHAR | The DuckDB schema containing the semantic view. |
| `semantic_view_name` | VARCHAR | The semantic view this dimension belongs to. |
| `table_name` | VARCHAR | The physical table name the dimension is scoped to (resolved from alias to actual table name). Empty string if no source table. |
| `name` | VARCHAR | The dimension name as declared in the DIMENSIONS clause. |
| `data_type` | VARCHAR | The inferred data type. Empty string if not resolved. |

**Key behavioral changes:**
- `expr` column removed -- dimension expressions are no longer exposed in SHOW output.
- `source_table` renamed to `table_name` and now shows the actual physical table name (e.g., `customers`) instead of the DDL alias (e.g., `c`). This is resolved via `alias_to_table_map()`.
- New `database_name` and `schema_name` columns prepended.

**Sections to update:**
- **Page intro:** Remove mention of "expression" from "name, expression, source table, and inferred data type" description.
- **Output Columns:** Replace 5-column table with 6-column table above.
- **Note about `data_type`:** Keep the note but verify it still applies (it does -- computed expressions may still show empty data type).
- **All example output tables:** Change to 6-column format. Replace alias values (`c`, `o`, `li`, `p`) in old `source_table` column with actual table names (`customers`, `orders`, `line_items`, `products`) in new `table_name` column. Remove `expr` column from all outputs.

Test-verified example output (from `test/sql/phase39_metadata_storage.test` line 112):
```
memory  main  p39_sv  p39_orders  order_id  (empty)
```

---

### Page 3: SHOW SEMANTIC METRICS (`docs/reference/show-semantic-metrics.rst`)

**Schema change:** Same as DIMENSIONS -- 5 columns -> 6 columns.

Old columns: `semantic_view_name`, `name`, `expr`, `source_table`, `data_type`

New columns (verified from `src/ddl/show_metrics.rs` lines 44-62, identical schema to show_dims.rs):

| Column | Type | Description |
|--------|------|-------------|
| `database_name` | VARCHAR | The DuckDB database containing the semantic view. |
| `schema_name` | VARCHAR | The DuckDB schema containing the semantic view. |
| `semantic_view_name` | VARCHAR | The semantic view this metric belongs to. |
| `table_name` | VARCHAR | The physical table name the metric is scoped to. Empty string for derived metrics. |
| `name` | VARCHAR | The metric name as declared in the METRICS clause. |
| `data_type` | VARCHAR | The inferred data type. Empty string if not resolved. |

**Key behavioral changes (same as DIMENSIONS, plus):**
- Derived metrics show empty `table_name` (was empty `source_table`).
- `expr` column removed -- the example showing derived metric expressions (`revenue - cost`, `profit / revenue * 100`) must be reworked. Derived metrics are now only distinguishable from base metrics by their empty `table_name`.

**Sections to update:**
- **Page intro:** Remove "aggregate expression" from description.
- **Output Columns:** Replace with 6-column table.
- **Derived metrics example:** Rework to show how derived metrics appear without `expr` column (empty `table_name` is the distinguishing feature).
- **All example output tables:** 6-column format, aliases -> actual table names, remove `expr` column.

Test-verified example output (from `test/sql/phase39_metadata_storage.test` line 122):
```
memory  main  p39_sv  p39_orders  total_amount  (empty)
```

---

### Page 4: SHOW SEMANTIC FACTS (`docs/reference/show-semantic-facts.rst`)

**Schema change:** 4 columns -> 6 columns.

Old columns: `semantic_view_name`, `name`, `expr`, `source_table`

New columns (verified from `src/ddl/show_facts.rs` lines 44-62, identical schema to show_dims.rs):

| Column | Type | Description |
|--------|------|-------------|
| `database_name` | VARCHAR | The DuckDB database containing the semantic view. |
| `schema_name` | VARCHAR | The DuckDB schema containing the semantic view. |
| `semantic_view_name` | VARCHAR | The semantic view this fact belongs to. |
| `table_name` | VARCHAR | The physical table name the fact is scoped to. |
| `name` | VARCHAR | The fact name as declared in the FACTS clause. |
| `data_type` | VARCHAR | The inferred data type (via typeof when table data exists). Empty string if not resolved. |

**Key behavioral changes:**
- `expr` column removed.
- `source_table` renamed to `table_name` (actual table name, not alias).
- **`data_type` column ADDED.** Facts now have data type inference. The old note stating "Unlike SHOW SEMANTIC DIMENSIONS and SHOW SEMANTIC METRICS, the facts output does not include a `data_type` column" is **now incorrect** and must be removed.
- All three SHOW commands (DIMENSIONS, METRICS, FACTS) now share the exact same 6-column schema.

Test-verified example with data_type populated (from `test/sql/phase39_metadata_storage.test` line 104):
```
memory  main  p39_sv  p39_orders  unit_price  DOUBLE
```

**Sections to update:**
- **Page intro:** Update description to mention data type.
- **Output Columns:** Replace 4-column table with 6-column table.
- **Remove the `.. note::` block** about facts not having `data_type`.
- **All example output tables:** 6-column format, aliases -> actual table names, add data_type column (empty or populated).

---

### Page 5: SHOW SEMANTIC DIMENSIONS FOR METRIC (`docs/reference/show-semantic-dimensions-for-metric.rst`)

**Schema change:** Completely restructured -- 5 columns -> 4 columns.

Old columns: `semantic_view_name`, `name`, `expr`, `source_table`, `data_type` (same as SHOW DIMS)

New columns (verified from `src/ddl/show_dims_for_metric.rs` lines 169-177):

| Column | Type | Description |
|--------|------|-------------|
| `table_name` | VARCHAR | The physical table name the dimension is scoped to. |
| `name` | VARCHAR | The dimension name. |
| `data_type` | VARCHAR | The inferred data type. Empty string if not resolved. |
| `required` | BOOLEAN | Always `false`. Reserved for future Snowflake parity. |

**Key behavioral changes:**
- `semantic_view_name` column removed (redundant -- the view is already specified in the IN clause).
- `expr` column removed.
- `source_table` renamed to `table_name` (actual table name).
- New `required` BOOLEAN column added (constant `false` for all rows).
- **No longer shares schema with SHOW SEMANTIC DIMENSIONS.** The existing note "same schema as SHOW SEMANTIC DIMENSIONS" and the `(same schema as ...)` parenthetical in Output Columns must be removed.

Test-verified example output (from `test/sql/phase34_1_show_dims_for_metric.test` lines 31-34):
```
p34fm_sales  product  (empty)  false
p34fm_sales  region   (empty)  false
```

Multi-table fan-trap filtered output (lines 89-92):
```
p34fm_customers  customer_country  (empty)  false
p34fm_customers  customer_name     (empty)  false
```

**Sections to update:**
- **Output Columns:** Replace 5-column table with 4-column table. Remove "same schema" cross-reference.
- **All example output tables:** 4-column format with `table_name`, `name`, `data_type`, `required`. Replace aliases with actual table names. Add `false` in required column for every row.
- **Fan Trap Filtering section:** The explanation of fan-trap logic remains unchanged (it is behavioral, not schema-related). Only the example outputs within this section need column updates.
- **Derived metrics example:** Update output columns.

---

### Page 6: DESCRIBE SEMANTIC VIEW (`docs/reference/describe-semantic-view.rst`)

**Schema change:** Complete format change -- single-row JSON blob -> multi-row property-per-row.

Old format: 1 row with 6 VARCHAR columns containing JSON arrays (`name`, `base_table`, `dimensions`, `metrics`, `joins`, `facts`)

New format (verified from `src/ddl/describe.rs` lines 325-341): Multiple rows with 5 VARCHAR columns:

| Column | Type | Description |
|--------|------|-------------|
| `object_kind` | VARCHAR | The type of object: `TABLE`, `RELATIONSHIP`, `FACT`, `DIMENSION`, `METRIC`, or `DERIVED_METRIC`. |
| `object_name` | VARCHAR | The name of the object (table name, relationship name, dimension/fact/metric name). |
| `parent_entity` | VARCHAR | The parent table for this object. Empty string for TABLE objects and DERIVED_METRIC objects. |
| `property` | VARCHAR | The property name being described. |
| `property_value` | VARCHAR | The property value. |

**Object kinds and their properties** (verified from `src/ddl/describe.rs`):

- **TABLE** (lines 86-118): `BASE_TABLE_DATABASE_NAME`, `BASE_TABLE_SCHEMA_NAME`, `BASE_TABLE_NAME`, `PRIMARY_KEY` (only emitted when PK declared; value is JSON array like `["id"]`)
- **RELATIONSHIP** (lines 147-175): `TABLE`, `REF_TABLE`, `FOREIGN_KEY` (JSON array), `REF_KEY` (JSON array)
- **FACT** (lines 194-216): `TABLE`, `EXPRESSION`, `DATA_TYPE`
- **DIMENSION** (lines 235-255): `TABLE`, `EXPRESSION`, `DATA_TYPE`
- **METRIC** (lines 285-308): `TABLE`, `EXPRESSION`, `DATA_TYPE`
- **DERIVED_METRIC** (lines 270-308): `EXPRESSION`, `DATA_TYPE` only (no TABLE property)

**Rows appear in definition order:** TABLEs, then RELATIONSHIPs, then FACTs, then DIMENSIONs, then METRICs/DERIVED_METRICs.

Test-verified example (from `test/sql/phase41_describe.test` lines 31-43, simple single-table view):
```
TABLE       p41_orders  (empty)     BASE_TABLE_DATABASE_NAME  memory
TABLE       p41_orders  (empty)     BASE_TABLE_SCHEMA_NAME    main
TABLE       p41_orders  (empty)     BASE_TABLE_NAME           p41_orders
TABLE       p41_orders  (empty)     PRIMARY_KEY               ["id"]
DIMENSION   region      p41_orders  TABLE                     p41_orders
DIMENSION   region      p41_orders  EXPRESSION                o.region
DIMENSION   region      p41_orders  DATA_TYPE                 (empty)
METRIC      total       p41_orders  TABLE                     p41_orders
METRIC      total       p41_orders  EXPRESSION                SUM(o.amount)
METRIC      total       p41_orders  DATA_TYPE                 (empty)
```

Test-verified example with derived metrics (lines 155-169):
```
...
METRIC          revenue  p41_orders  TABLE       p41_orders
METRIC          revenue  p41_orders  EXPRESSION  SUM(o.amount)
METRIC          revenue  p41_orders  DATA_TYPE   (empty)
DERIVED_METRIC  profit   (empty)     EXPRESSION  revenue * 0.3
DERIVED_METRIC  profit   (empty)     DATA_TYPE   (empty)
```

**Sections requiring complete rewrite:**
- **Meta description:** Change from "single-row JSON result" to "property-per-row format showing each object and its properties".
- **Page intro:** Change from "Returns the definition of a semantic view as a single-row result set" to describe multi-row property-per-row output.
- **Output Columns:** Complete replacement with 5-column table above, plus the object kinds and properties breakdown.
- **Tip about JSON parsing:** Remove entirely. The old tip about `json_extract` on JSON blob columns is no longer relevant. Replace with guidance on filtering by `object_kind`: `SELECT * FROM (DESCRIBE SEMANTIC VIEW sv) WHERE object_kind = 'DIMENSION';`
- **All examples:** Complete replacement showing multi-row property-per-row output. Show examples for: simple single-table view, multi-table view with relationships, view with facts, view with derived metrics.

---

## API Reference Status

- **Type:** Manual reference pages (`api_reference: manual` in config.yaml)
- **Status:** All 6 pages exist and need content updates only (no new pages)

## Audience Targeting

All 6 pages target the single configured persona: **Data engineers exploring semantic views** (intermediate skill level). These are SQL reference pages -- the audience knows SQL and DuckDB basics, needs accurate column schemas and realistic output examples.

## Coverage Gaps

- **Non-reference pages:** Other documentation pages (tutorials, how-to guides) that show SHOW or DESCRIBE output may also contain outdated examples. A broader audit after these 6 pages are updated is recommended.
- **Version markers:** Consider adding `.. versionchanged:: 0.5.5` admonitions to each page to mark the schema changes for readers upgrading from earlier versions.
- **error-messages.rst:** Error messages for these commands have not changed in v0.5.5, so that page does not need updating for this pass.

## Notes

- The existing reference pages use `sqlgrammar` as the code-block language for syntax sections.
- Pages use `:ref:` labels with `ref-` prefix for cross-referencing.
- The SHOW SEMANTIC DIMENSIONS FOR METRIC page (page 5) has extensive fan-trap filtering examples that need column updates but the behavioral explanation of fan-trap logic remains correct.
- All column schemas were verified against the actual Rust source code (`src/ddl/*.rs`) and integration test expected output (`test/sql/phase39_metadata_storage.test`, `test/sql/phase41_describe.test`, `test/sql/phase34_1_show_dims_for_metric.test`).
