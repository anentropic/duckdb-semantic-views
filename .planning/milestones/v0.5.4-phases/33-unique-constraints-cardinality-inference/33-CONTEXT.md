# Phase 33: UNIQUE Constraints & Cardinality Inference - Context

**Gathered:** 2026-03-15
**Status:** Ready for planning

<domain>
## Phase Boundary

Replace explicit cardinality keywords (MANY TO ONE, ONE TO MANY, ONE TO ONE, MANY TO MANY) with Snowflake-style cardinality inference from PK/UNIQUE constraint declarations. Users declare UNIQUE constraints on tables in the TABLES clause; the extension infers relationship cardinality automatically. No explicit cardinality keywords in DDL.

</domain>

<decisions>
## Implementation Decisions

### Old keyword rejection
- No special handling for old cardinality keywords — the parser simply doesn't recognize them anymore
- Standard "unexpected token" error fires if someone writes MANY TO ONE etc. in new DDL
- No migration hints, no deprecation warnings — pretend the old syntax never existed

### Backward compatibility
- No backward compatibility for stored definitions created with v0.5.3 or earlier
- Old JSON with explicit cardinality fields is REJECTED on load (not silently accepted)
- Detect old-format JSON and return a clear human-readable error: "This semantic view was created with an older version. Please recreate it with the new DDL syntax."
- This is a clean break — users must recreate semantic views after upgrading

### Cardinality enum
- Remove `OneToMany` variant from the `Cardinality` enum entirely
- Remove `ManyToMany` variant (CARD-06) — never existed in code but ensure it stays absent
- Cardinality becomes a two-variant enum: `ManyToOne` and `OneToOne`
- Fan trap detection handles direction explicitly (forward ManyToOne = safe, reverse ManyToOne = fan-out)

### Cardinality inference (Snowflake-aligned)
- Every FK must reference a declared PK or UNIQUE constraint on the target table (CARD-03) — error at define time if not
- Cardinality is inferred from the FK side's constraints:
  - FK columns match a PK or UNIQUE on the FK-side (from_alias) table → `OneToOne`
  - FK columns are bare (no PK/UNIQUE on FK-side table) → `ManyToOne`
- No "bare FK default" — an unresolvable FK reference is always a define-time error
- Tables without PK or UNIQUE can exist (e.g., fact tables) but cannot be REFERENCES targets

### REFERENCES column syntax
- Adopt Snowflake's exact REFERENCES syntax with optional target column list
- `from_alias(fk_cols) REFERENCES target` — resolves to target's PRIMARY KEY
- `from_alias(fk_cols) REFERENCES target(ref_cols)` — resolves to named PK or UNIQUE on target
- FK column count must exactly match referenced column count (positional mapping: a→x, b→y)
- `REFERENCES target` with no column list when target has no PRIMARY KEY declared → define-time error: "Table 'target' has no PRIMARY KEY. Specify referenced columns explicitly: REFERENCES target(col)."

### DESCRIBE output
- DESCRIBE SEMANTIC VIEW shows UNIQUE constraints alongside PRIMARY KEY info in tables section
- DESCRIBE shows inferred cardinality on relationships (e.g., "order_customer: orders.customer_id -> customers.id [many-to-one]")

### Error messages
- Fan trap errors use inference language: "Relationship 'X' has many-to-one cardinality (inferred: FK is not PK/UNIQUE). Querying dimension 'D' from the many-side would inflate aggregation results."
- CARD-03 validation errors show available constraints: "FK (order_id) on 'orders' does not match any PRIMARY KEY or UNIQUE constraint on 'customers'. Available: PK(id), UNIQUE(email)."

### Claude's Discretion
- Exact placement of cardinality inference logic (parse.rs vs graph.rs vs define.rs)
- Internal representation of UNIQUE constraints in the model
- Serde strategy for detecting old-format JSON
- Fan trap code refactoring to work with two-variant cardinality
- Test structure and coverage approach

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Snowflake reference design
- `_notes/semantic-views-duckdb-design-doc.md` — Prior art analysis including Snowflake semantic views, architecture decisions
- Snowflake CREATE SEMANTIC VIEW docs: https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view — REFERENCES syntax, PK/UNIQUE constraint rules

### Requirements
- `.planning/REQUIREMENTS.md` — CARD-01 through CARD-09 requirements for this phase

### Current model and parser
- `src/model.rs` — `Cardinality` enum (line ~93), `TableRef` struct (line ~7), `Join` struct (line ~120)
- `src/body_parser.rs` — `parse_single_table_entry` (line ~407), `parse_cardinality_tokens` (line ~624), `parse_single_relationship_entry` (line ~666), `find_primary_key` (line ~519)
- `src/graph.rs` — `validate_graph` (line ~295), `check_fk_pk_counts` (line ~219)
- `src/expand.rs` — `check_fan_traps` (line ~987), cardinality direction checks in `check_path_up`/`check_path_down`
- `src/parse.rs` — `rewrite_ddl_keyword_body` (line ~449), where SemanticViewDefinition is assembled

### Existing tests
- `test/sql/phase26_join_resolution.test` — PK/FK ON clause synthesis, graph validation
- `test/sql/phase31_fan_trap.test` — Fan trap detection with explicit cardinality
- `test/sql/phase32_role_playing.test` — Role-playing dims, USING relationships

### Tech debt
- `TECH-DEBT.md` — Accepted decisions and deferred items

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Cardinality` enum with `#[serde(default)]` and `is_default()` — pattern for serde backward compat (though we're breaking compat this time)
- `find_keyword_ci` in body_parser.rs — case-insensitive keyword finder, reusable for parsing `UNIQUE`
- `check_fk_pk_counts` in graph.rs — validates FK/PK column counts, base for new PK/UNIQUE matching validation
- `card_map` in expand.rs — HashMap mapping (from, to) to cardinality, used by fan trap detection

### Established Patterns
- Serde `#[serde(default, skip_serializing_if = "Vec::is_empty")]` on Vec fields for optional data
- `find_primary_key` pattern for parsing keyword + parenthesized column list — reuse for UNIQUE parsing
- Kahn's algorithm (toposort) used in both graph.rs and expand.rs for cycle detection
- Define-time validation in graph.rs before persistence — new validations go here

### Integration Points
- `parse_single_table_entry` returns `TableRef` — must be extended with `unique_constraints`
- `parse_single_relationship_entry` returns `Join` — cardinality field now populated by inference, not parsing
- `rewrite_ddl_keyword_body` in parse.rs — where tables + relationships are assembled; inference can run here
- `validate_graph` in graph.rs — where new CARD-03/CARD-09 validation goes
- `check_fan_traps` in expand.rs — reads `j.cardinality` unchanged; just needs OneToMany variant removal handled

</code_context>

<specifics>
## Specific Ideas

- "Pretend the old syntax never existed" — no migration hints, no deprecation, just standard parse errors for unrecognized tokens
- Snowflake-aligned inference model: referenced side must have PK/UNIQUE, cardinality inferred from FK side
- Clean break on backward compat: detect old format, give clear "recreate your view" error

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 33-unique-constraints-cardinality-inference*
*Context gathered: 2026-03-15*
