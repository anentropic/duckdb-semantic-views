# Phase 31: Fan Trap Detection - Context

**Gathered:** 2026-03-14
**Status:** Ready for planning
**Source:** User discussion (Snowflake comparison)

<domain>
## Phase Boundary

Add optional cardinality declarations to relationships and block queries that would produce inflated aggregation results due to one-to-many fan-out.

</domain>

<decisions>
## Implementation Decisions

### Cardinality Syntax
- Cardinality is declared after REFERENCES using plain SQL keywords: `MANY TO ONE`, `ONE TO ONE`, `ONE TO MANY`
- No `CARDINALITY` keyword prefix — just the cardinality type directly
- Example: `order_to_customer AS o(customer_id) REFERENCES c MANY TO ONE`
- When omitted, defaults to `MANY TO ONE` (most common FK pattern)
- No underscores in the keywords (not `MANY_TO_ONE`)

### Fan Trap Behavior
- **Block the query** when a metric aggregates across a one-to-many boundary (Snowflake-style)
- Query fails with a descriptive error explaining the fan trap risk
- This is a hard error, not a warning — prevents inflated results
- Deviation from original FAN-02/FAN-03 which specified warnings; user chose blocking after reviewing trade-offs

### Cardinality Values
- `MANY TO ONE` — standard FK pattern (many rows reference one PK row). Default.
- `ONE TO ONE` — unique FK (1:1 mapping between tables)
- `ONE TO MANY` — reverse direction (PK side declares it has many referencing rows)
- No `MANY TO MANY` support (matches Snowflake — not supported)

### Fan Trap Detection Logic
- A fan trap occurs when a metric's source table is on the "one" side of a one-to-many relationship, and a dimension's source table is on the "many" side, forcing the metric values to be duplicated per dimension row
- More precisely: when the join path from metric source to dimension source crosses a one-to-many edge in the fan-out direction
- Detection happens at query expansion time (in expand.rs), not at DDL time

### Claude's Discretion
- Error message format and wording
- Internal representation of cardinality enum
- Graph traversal algorithm for detecting fan traps
- How to handle chains of relationships (transitive fan-out detection)

</decisions>

<specifics>
## Specific Ideas

- Parser extension: after `REFERENCES <alias>`, optionally consume `MANY TO ONE` / `ONE TO ONE` / `ONE TO MANY` tokens
- Add `Cardinality` enum to the `Join` struct in model.rs
- Fan trap check in expand.rs: after resolving joins, walk the join graph and check if any metric source is on the "one" side of a one-to-many edge relative to any dimension source
- Error message should name the specific relationship and tables involved

</specifics>

<deferred>
## Deferred Ideas

- Smart per-aggregate-type detection (block SUM/COUNT but allow MIN/MAX) — future enhancement
- `SHOW SEMANTIC DIMENSIONS FOR METRIC` compatibility query (Snowflake feature)
- Configurable warn vs block via pragma
- `MANY TO MANY` support via bridge tables

</deferred>

---

*Phase: 31-fan-trap-detection*
*Context gathered: 2026-03-14 via user discussion*
