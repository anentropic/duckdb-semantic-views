# Phase 26: PK/FK Join Resolution - Context

**Gathered:** 2026-03-13
**Status:** Ready for planning

<domain>
## Phase Boundary

Synthesize correct SQL JOIN ON clauses from PK/FK declarations stored in Phase 24 model fields (`TableRef.pk_columns`, `Join.from_alias`, `Join.fk_columns`). Validate the relationship graph at define time (CREATE time). Determine join order via topological sort. No new DDL syntax changes — this phase consumes what Phase 24 (model) and Phase 25 (parser) already produce.

</domain>

<decisions>
## Implementation Decisions

### JOIN type
- All generated joins use LEFT JOIN — globally, for all definitions (old and new)
- This is a change from the current `expand.rs` which emits bare `JOIN` (INNER)
- Rationale: industry consensus — Snowflake auto-infers (but defaults to preserving rows), Databricks hardcodes LEFT OUTER JOIN, Cube.dev hardcodes LEFT JOIN
- Users who want INNER semantics can add a WHERE filter (e.g., `WHERE fk IS NOT NULL`)
- No join type configuration in DDL syntax — not exposed to users
- No cardinality inference at define or query time

### Graph validation (define-time, at CREATE)
- Relationship graph must be a tree rooted at the base table (first table in TABLES clause)
- Cycles rejected with path-naming error: "cycle detected in relationships: orders → customers → orders"
- Diamonds rejected with path-naming error: "diamond: two paths to 'products' via 'orders' and 'inventory'"
- Self-references explicitly rejected: "table 'employees' cannot reference itself" (distinct from cycle detection)
- Self-referencing relationships (employee→manager hierarchy) noted as future requirement

### Unreachable table handling (define-time, at CREATE)
- Every dim/metric's source_table alias must be reachable from the base table via the relationship graph — error at CREATE time if not
- Orphan tables (declared in TABLES but not connected by any relationship and not the base table) error at CREATE time
- Error messages use existing `strsim` fuzzy-match for "did you mean?" suggestions (consistent with parse.rs error style)

### Composite key / FK-PK matching
- FK columns positionally match PK columns: `o(customer_id) REFERENCES c` means `customer_id` maps to `c.pk_columns[0]`
- Error at CREATE time if FK column count != PK column count on the referenced table
- No explicit column naming on REFERENCES side (e.g., `REFERENCES c(id)` not supported) — no parser change needed
- Explicit REFERENCES columns deferred to when UNIQUE constraint support is added

### Claude's Discretion
- Topological sort algorithm choice (Kahn's vs DFS-based)
- Graph data structure (adjacency list, edge list, etc.)
- Where validation code lives (expand.rs, new module, or define.rs)
- Exact error message wording beyond the patterns specified above
- How to handle the transition from old `join_columns`/`on` fields to new PK/FK synthesis in `append_join_on_clause`

</decisions>

<specifics>
## Specific Ideas

- Snowflake semantic view DDL is the design reference — when in doubt, match Snowflake behavior
- Error messages should name the specific path/tables involved, not just "cycle detected" or "diamond detected"
- The existing `strsim::levenshtein` fuzzy-match system from `parse.rs` should be reused for "did you mean?" suggestions in graph validation errors

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `src/expand.rs:append_join_on_clause` (line 303): Currently handles `join_columns` (Phase 11.1) — needs updating to use `from_alias` + `fk_columns` → `TableRef.pk_columns` for ON clause synthesis
- `src/expand.rs:resolve_joins` (line 168): Fixed-point transitive resolution via `source_table` + `join.on` substring matching — needs replacing with graph-based traversal using relationship edges
- `src/expand.rs:suggest_closest` (line 12): Levenshtein fuzzy-match — reuse for graph validation error suggestions
- `src/expand.rs:quote_ident` / `quote_table_ref`: SQL identifier quoting — reuse in generated ON clauses

### Established Patterns
- Error position = byte offset into original query string (for parse errors)
- Case-insensitive matching via `eq_ignore_ascii_case` throughout
- `strsim::levenshtein` for fuzzy suggestions (already a Cargo dep)
- No new Cargo dependencies — hand-written algorithms

### Integration Points
- `src/expand.rs:expand()` (line 348): Main expansion function — JOIN generation at lines 421-437 needs updating from bare `JOIN` to `LEFT JOIN` and from `join_columns`-based ON to PK/FK-based ON
- `src/model.rs:Join` (line 77): Has Phase 24 fields `from_alias`, `fk_columns`, `name` — currently unused by expansion
- `src/model.rs:TableRef` (line 7): Has `pk_columns` — currently unused by expansion
- `src/ddl/define.rs`: `create_semantic_view()` — define-time graph validation should run here before persisting
- `src/body_parser.rs:parse_single_relationship_entry` (line 577): Produces `Join` with `from_alias`, `fk_columns`, `table` (to_alias) — these are the inputs for graph building

</code_context>

<deferred>
## Deferred Ideas

- Self-referencing relationships (employee→manager hierarchy) — requires role-playing dimension support (related to ADV-05)
- Explicit column naming on REFERENCES side (`REFERENCES c(id)`) — deferred to UNIQUE constraint support phase
- Join type configuration in DDL syntax — industry consensus is no configuration; revisit only if user demand arises
- Cardinality inference from PK/UNIQUE metadata — Snowflake does this; we don't have the metadata infrastructure yet

</deferred>

---

*Phase: 26-pk-fk-join-resolution*
*Context gathered: 2026-03-13*
