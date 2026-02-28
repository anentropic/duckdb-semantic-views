# Phase 2: Storage and DDL - Context

**Gathered:** 2026-02-24
**Status:** Ready for planning

<domain>
## Phase Boundary

Users can register, inspect, and remove semantic view definitions via DuckDB scalar/table functions (`define_semantic_view`, `drop_semantic_view`, `list_semantic_views`, `describe_semantic_view`), and those definitions survive a DuckDB restart. No query expansion in this phase — that's Phase 3.

</domain>

<decisions>
## Implementation Decisions

### Definition JSON schema
- Flat object with typed arrays (not nested entity structure)
- Required top-level fields: `base_table` (string), `dimensions` (array), `metrics` (array)
- Optional top-level fields: `filters` (array, defaults to []), `joins` (array, defaults to [])
- Dimension item shape: `{ "name": "region", "expr": "region" }` — name + expr only, no label/description
- Metric item shape: `{ "name": "revenue", "expr": "sum(amount)" }` — name + expr only
- Join item shape: `{ "table": "customers", "on": "orders.customer_id = customers.id" }` — table + raw ON expression
- Validation at define time: error immediately on invalid JSON or missing required fields; nothing written to catalog on failure

### Error contracts
- `define_semantic_view` with a name that already exists: **error** — user must call `drop_semantic_view` first. No silent overwrites.
- `drop_semantic_view` with a name that doesn't exist: **error** with message "semantic view 'X' does not exist"
- `describe_semantic_view` with a name that doesn't exist: **error** with message "semantic view 'X' does not exist"
- Success confirmation format: single VARCHAR column with human-readable message (e.g., "Semantic view 'orders' registered successfully") — standard scalar function pattern

### `describe_semantic_view` output shape
- Returns **one row** with typed columns: `(name VARCHAR, base_table VARCHAR, dimensions VARCHAR/JSON, metrics VARCHAR/JSON, filters VARCHAR/JSON, joins VARCHAR/JSON)`
- JSON columns: use DuckDB JSON type if duckdb-rs makes this straightforward; fall back to VARCHAR if not — Claude's discretion
- `list_semantic_views()` returns lightweight directory: `(name VARCHAR, base_table VARCHAR)` only — users call describe for full details

### Catalog table placement and sync
- Catalog lives at `semantic_layer._definitions` — extension creates `semantic_layer` schema and `_definitions` table on every extension load (CREATE SCHEMA/TABLE IF NOT EXISTS — idempotent)
- Table schema: `(name VARCHAR PRIMARY KEY, definition JSON)`
- In-memory HashMap loaded from catalog at extension load time (SELECT all rows after table creation)
- Write order for DDL mutations: write to catalog first (returns a Rust `Result`), then update HashMap only on success. If catalog write fails, error is returned to user and HashMap is not touched — no stale state possible on write failure.
- Catalog is authoritative source; HashMap is a load-time cache. Drift recovery: reload from catalog on next extension load.

</decisions>

<specifics>
## Specific Ideas

- Requirements say `_semantic_views_catalog` (flat) but user confirmed `semantic_layer._definitions` (schema-namespaced) — use the schema-namespaced approach.
- The write-catalog-first + only-update-HashMap-on-success pattern maps naturally to Rust's `Result` propagation — no special invalidation logic needed.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 02-storage-and-ddl*
*Context gathered: 2026-02-24*
