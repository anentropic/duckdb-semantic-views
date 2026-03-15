# Phase 33: UNIQUE Constraints & Cardinality Inference - Research

**Researched:** 2026-03-15
**Domain:** Rust model/parser/validation changes for constraint-based cardinality inference
**Confidence:** HIGH

## Summary

Phase 33 replaces explicit cardinality keywords with Snowflake-style cardinality inference from PK/UNIQUE constraint declarations. This is a breaking change by design: old DDL with cardinality keywords will not parse, and old stored JSON with explicit cardinality fields will be rejected on load.

The implementation touches five files in a well-understood dependency chain: `model.rs` (data structures) -> `body_parser.rs` (parsing UNIQUE, removing cardinality keywords) -> `parse.rs` (inference logic at DDL assembly time) -> `graph.rs` (new validations: CARD-03, CARD-09 FK/PK/UNIQUE matching) -> `expand.rs` (fan trap detection adaptation, ON clause synthesis with UNIQUE-referenced columns). The DESCRIBE output in `ddl/describe.rs` also needs updating to show UNIQUE constraints and inferred cardinality.

**Primary recommendation:** Implement in model-first order: (1) extend model with UNIQUE constraints and ref_columns on Join, (2) update parser to handle UNIQUE syntax and remove cardinality keywords, (3) add inference logic, (4) update validations, (5) adapt fan trap detection and ON clause synthesis.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- No special handling for old cardinality keywords -- the parser simply does not recognize them anymore. Standard "unexpected token" error fires if someone writes MANY TO ONE etc. in new DDL. No migration hints, no deprecation warnings.
- No backward compatibility for stored definitions created with v0.5.3 or earlier. Old JSON with explicit cardinality fields is REJECTED on load (not silently accepted). Detect old-format JSON and return a clear human-readable error: "This semantic view was created with an older version. Please recreate it with the new DDL syntax."
- Remove `OneToMany` variant from the `Cardinality` enum entirely. Remove `ManyToMany` variant (CARD-06) -- never existed in code but ensure it stays absent. Cardinality becomes a two-variant enum: `ManyToOne` and `OneToOne`.
- Every FK must reference a declared PK or UNIQUE constraint on the target table (CARD-03) -- error at define time if not. Tables without PK or UNIQUE can exist (e.g., fact tables) but cannot be REFERENCES targets.
- Cardinality is inferred from the FK side's constraints: FK columns match a PK or UNIQUE on the FK-side (from_alias) table -> `OneToOne`; FK columns are bare (no PK/UNIQUE on FK-side table) -> `ManyToOne`.
- Adopt Snowflake's exact REFERENCES syntax: `from_alias(fk_cols) REFERENCES target` resolves to target's PRIMARY KEY; `from_alias(fk_cols) REFERENCES target(ref_cols)` resolves to named PK or UNIQUE on target. FK column count must exactly match referenced column count.
- DESCRIBE SEMANTIC VIEW shows UNIQUE constraints alongside PRIMARY KEY info in tables section and shows inferred cardinality on relationships.
- Fan trap errors use inference language: "Relationship 'X' has many-to-one cardinality (inferred: FK is not PK/UNIQUE)."
- CARD-03 validation errors show available constraints: "FK (order_id) on 'orders' does not match any PRIMARY KEY or UNIQUE constraint on 'customers'. Available: PK(id), UNIQUE(email)."
- This is a clean break -- users must recreate semantic views after upgrading.

### Claude's Discretion
- Exact placement of cardinality inference logic (parse.rs vs graph.rs vs define.rs)
- Internal representation of UNIQUE constraints in the model
- Serde strategy for detecting old-format JSON
- Fan trap code refactoring to work with two-variant cardinality
- Test structure and coverage approach

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CARD-01 | TABLES clause supports `UNIQUE (col, ...)` constraint alongside existing `PRIMARY KEY (col)` | Model: add `unique_constraints: Vec<Vec<String>>` to `TableRef`. Parser: add `find_unique` scanner parallel to `find_primary_key`. |
| CARD-02 | A table can have one PRIMARY KEY and multiple UNIQUE constraints (composite allowed) | Vec<Vec<String>> supports multiple composite UNIQUE constraints. Parser iterates after PK to collect zero or more UNIQUE clauses. |
| CARD-03 | Referenced columns in RELATIONSHIPS must match a declared PRIMARY KEY or UNIQUE constraint on the target table | New validation function in `graph.rs`: iterate joins, for each join find target table, check that FK-referenced columns match PK or one of the UNIQUE constraints exactly (set equality, case-insensitive). |
| CARD-04 | Cardinality inferred from constraints: FK column has PK/UNIQUE = one-to-one; FK column bare = many-to-one | Inference logic runs after parsing relationships: check if FK columns match any PK/UNIQUE on the from_alias table. Store inferred cardinality on `Join.cardinality`. |
| CARD-05 | Explicit cardinality keywords removed from parser | Delete `parse_cardinality_tokens` function. In `parse_single_relationship_entry`, after REFERENCES target, no more tokens are expected (or only `(ref_cols)` paren list). |
| CARD-06 | ManyToMany variant removed from Cardinality enum | Already absent from code. OneToMany also removed per user decision, leaving two-variant enum. |
| CARD-07 | `REFERENCES target` (no column list) resolves to target's PRIMARY KEY; `REFERENCES target(col)` resolves to matching PK or UNIQUE | Parser change: after REFERENCES, check for `(` to distinguish bare vs column-list form. Store resolved ref_columns on Join model. |
| CARD-08 | Fan trap detection continues to work using inferred cardinality values | Remove `Cardinality::OneToMany` branches from `check_path_up`/`check_path_down`. Two-variant enum: ManyToOne forward=safe, ManyToOne reverse=fan-out, OneToOne=always safe. |
| CARD-09 | Composite FK referencing a subset of a composite PK is rejected | Covered by CARD-03 validation: exact set match required, not subset. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde / serde_json | 1.x | Model serialization/deserialization | Already in use; UNIQUE constraints and ref_columns use same serde patterns |
| strsim | 0.11 | Levenshtein distance for error suggestions | Already in use for "did you mean?" hints |

### Supporting
No new dependencies required. All changes are within existing Rust source files using established patterns.

## Architecture Patterns

### Recommended Change Topology

```
src/
├── model.rs          # (1) Add UNIQUE to TableRef, ref_columns to Join, remove OneToMany
├── body_parser.rs    # (2) Parse UNIQUE clauses, remove cardinality keywords, parse REFERENCES(cols)
├── parse.rs          # (3) Run cardinality inference after parsing, before JSON serialization
├── graph.rs          # (4) New validations: FK-ref matching (CARD-03/09), old-JSON detection
├── expand.rs         # (5) Adapt fan trap to 2-variant enum, ON clause uses ref_columns
└── ddl/
    └── describe.rs   # (6) Show UNIQUE constraints and inferred cardinality
```

### Pattern 1: Model Extension for UNIQUE Constraints

**What:** Add `unique_constraints` field to `TableRef` to store zero or more UNIQUE constraint column lists.

**When to use:** This follows the exact pattern used for `pk_columns` (Phase 24).

**Example:**
```rust
// src/model.rs - TableRef extension
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableRef {
    pub alias: String,
    pub table: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pk_columns: Vec<String>,
    /// UNIQUE constraints on this table. Each inner Vec is one constraint's columns.
    /// A table can have zero or more UNIQUE constraints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_constraints: Vec<Vec<String>>,
}
```

### Pattern 2: Referenced Columns on Join

**What:** Add `ref_columns` field to `Join` to store the resolved referenced columns on the target side.

**Why:** Currently `synthesize_on_clause` zips FK columns with the target table's `pk_columns`. With CARD-07, the FK may reference a UNIQUE constraint instead. Storing `ref_columns` on the Join decouples ON clause synthesis from needing to re-resolve which constraint was referenced.

**Example:**
```rust
// src/model.rs - Join extension
pub struct Join {
    // ... existing fields ...

    /// Resolved referenced columns on the target table.
    /// Populated during inference: either the target's PK or the explicit UNIQUE columns.
    /// Used by synthesize_on_clause to generate ON clause.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ref_columns: Vec<String>,

    // cardinality stays but is now always inferred, never parsed
    #[serde(default, skip_serializing_if = "Cardinality::is_default")]
    pub cardinality: Cardinality,
}
```

### Pattern 3: Cardinality Inference Logic Placement

**Recommendation:** Place cardinality inference in `parse.rs::rewrite_ddl_keyword_body` after `parse_keyword_body` returns but before constructing the `SemanticViewDefinition`.

**Why this location:**
- The parsed `KeywordBody` has all tables (with PK/UNIQUE) and relationships available.
- Inference needs both tables and relationships together.
- `graph.rs::validate_graph` runs in `define.rs::bind()` AFTER JSON deserialization, so inference must happen before serialization.
- Keeping inference in `parse.rs` means the JSON written to the catalog already has correct cardinality and ref_columns -- no second pass needed.

**Alternative considered:** Running inference in `graph.rs::validate_graph`. Rejected because `validate_graph` receives a `&SemanticViewDefinition` (immutable reference) and would need to return modified data, breaking its validation-only contract.

**Example:**
```rust
// src/parse.rs - in rewrite_ddl_keyword_body, after parse_keyword_body
fn rewrite_ddl_keyword_body(...) -> Result<Option<String>, ParseError> {
    let mut keyword_body = parse_keyword_body(body_text, body_offset)?;

    // Infer cardinality and resolve ref_columns
    infer_cardinality(&keyword_body.tables, &mut keyword_body.relationships)?;

    // ... construct SemanticViewDefinition and serialize ...
}
```

### Pattern 4: Old-Format JSON Detection

**What:** Detect stored JSON from v0.5.3 that has explicit cardinality values or lacks the new fields, and reject it with a clear error.

**Strategy:** Use serde's `#[serde(deny_unknown_fields)]` -- no, that's too aggressive since the model explicitly allows unknown fields. Instead, add a post-deserialization check:

**Recommended approach:** After `from_json` deserializes successfully, check for sentinel conditions that indicate old-format JSON:
1. If any `Join` has a `cardinality` value but `ref_columns` is empty AND `fk_columns` is non-empty, it was created before Phase 33.
2. Alternatively: add a version marker to the JSON (e.g., `"schema_version": 2`).

**Simplest approach:** Check for absence of `ref_columns` on any join that has `fk_columns`. New Phase 33 definitions always populate `ref_columns`. Old definitions never have it.

```rust
// In define.rs bind(), after from_json but before validate_graph
fn check_not_legacy_format(def: &SemanticViewDefinition) -> Result<(), String> {
    for join in &def.joins {
        if !join.fk_columns.is_empty() && join.ref_columns.is_empty() {
            return Err(
                "This semantic view was created with an older version. \
                 Please recreate it with the new DDL syntax.".to_string()
            );
        }
    }
    Ok(())
}
```

### Pattern 5: UNIQUE Parsing in body_parser.rs

**What:** After parsing PRIMARY KEY in `parse_single_table_entry`, continue scanning for zero or more `UNIQUE (col, ...)` clauses.

**Reuse:** The `find_primary_key` function pattern can be adapted to create `find_unique` which finds `UNIQUE` as a word boundary match. The `extract_paren_content` function is already available for extracting column lists.

**Parsing flow:**
```
alias AS table_name PRIMARY KEY (pk_cols) [UNIQUE (u1_cols) [UNIQUE (u2_cols) ...]]
```

After consuming PRIMARY KEY and its paren list, the parser scans the remainder for UNIQUE keywords. Each UNIQUE is followed by a parenthesized column list.

**Important:** PRIMARY KEY is currently required (parser errors if not found). This should remain -- every table must have a PK. UNIQUE constraints are optional, zero or more.

### Pattern 6: REFERENCES Column List Parsing

**What:** Extend `parse_single_relationship_entry` to handle optional column list after the target alias.

**Current flow (body_parser.rs lines 666-759):**
```
rel_name AS from_alias(fk_cols) REFERENCES to_alias [MANY TO ONE | ONE TO ONE | ONE TO MANY]
```

**New flow:**
```
rel_name AS from_alias(fk_cols) REFERENCES to_alias[(ref_cols)]
```

After finding REFERENCES and consuming the target alias token, check if the next non-whitespace character is `(`. If so, extract the column list. If not, the target's PK is used (resolved during inference).

The cardinality tokens (`MANY TO ONE`, etc.) are no longer expected -- anything after the target alias (or its optional column list) is an error.

### Anti-Patterns to Avoid

- **Checking cardinality at expand time:** Cardinality inference MUST happen at define time (in parse.rs), not at query time. The stored JSON must contain the inferred cardinality.
- **Partial PK/UNIQUE matching:** CARD-09 explicitly requires exact match. Do NOT implement subset matching.
- **Silent acceptance of old JSON:** User decision explicitly requires rejection, not graceful degradation.
- **Storing cardinality as string:** Keep the enum. Two variants: `ManyToOne` (default), `OneToOne`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Keyword scanning | Custom regex engine | Existing `find_keyword_ci` function | Already handles word boundaries, case insensitivity |
| Parenthesized list parsing | Manual character scanning | Existing `extract_paren_content` | Already handles nested parens, string literals |
| Levenshtein suggestions | Custom edit distance | Existing `strsim::levenshtein` | Already in dependency tree |
| Column list splitting | Custom CSV parser | Existing `split_at_depth0_commas` | Already handles nested expressions |

**Key insight:** The body_parser.rs already has all the parsing primitives needed. The new UNIQUE and REFERENCES(cols) parsing is a composition of existing functions.

## Common Pitfalls

### Pitfall 1: ON Clause Synthesis Using Wrong Columns
**What goes wrong:** After adding UNIQUE-referenced relationships, `synthesize_on_clause` continues to zip FK columns with `pk_columns` instead of the resolved `ref_columns`.
**Why it happens:** The current code (`expand.rs:286-307`) hardcodes lookup of `pk_columns` from the target table.
**How to avoid:** Change `synthesize_on_clause_scoped` to use `join.ref_columns` (new field) instead of looking up `pk_columns` from the tables list. Fall back to `pk_columns` only when `ref_columns` is empty (legacy data -- though we're breaking backward compat, this is defensive).
**Warning signs:** Queries produce wrong results or "column not found" errors when the relationship references a UNIQUE constraint instead of the PK.

### Pitfall 2: Cardinality Inference Checking Wrong Table
**What goes wrong:** Inference checks whether FK columns are PK/UNIQUE on the TARGET table instead of the FROM (FK-side) table.
**Why it happens:** Confusion between two separate checks: (a) FK must reference PK/UNIQUE on TARGET (CARD-03), (b) cardinality is inferred from whether FK columns are PK/UNIQUE on FROM-SIDE (CARD-04).
**How to avoid:** Clearly separate the two checks in code:
1. `validate_fk_references_target` -- CARD-03: FK references must match a PK/UNIQUE on the target table.
2. `infer_cardinality_from_source` -- CARD-04: check if FK columns match PK/UNIQUE on the from_alias table.
**Warning signs:** All relationships inferred as OneToOne (checking target instead of source).

### Pitfall 3: Case-Insensitive Column Matching
**What goes wrong:** FK columns and PK/UNIQUE columns are compared case-sensitively, causing valid references to be rejected.
**Why it happens:** Column names in user DDL may have different casing than in constraint declarations.
**How to avoid:** Always compare column names case-insensitively (`.to_ascii_lowercase()`). This pattern is already used throughout the codebase for alias matching.
**Warning signs:** "FK does not match any PRIMARY KEY or UNIQUE constraint" errors when the columns differ only in casing.

### Pitfall 4: Composite UNIQUE Set Matching vs Positional Matching
**What goes wrong:** Using positional (index-based) matching instead of set matching for composite constraints.
**Why it happens:** PK matching currently uses positional zipping (FK[0]->PK[0], FK[1]->PK[1]).
**How to avoid:** For CARD-03 validation (does the FK reference a valid PK/UNIQUE?), use SET equality -- the FK columns as a set must exactly match a PK or UNIQUE column set. For ON clause synthesis, use POSITIONAL mapping -- FK[0]->ref[0], FK[1]->ref[1], because column order matters for the equijoin. The user controls positional mapping via column ordering in the REFERENCES clause.
**Warning signs:** Valid composite UNIQUE references rejected because columns are in different order.

### Pitfall 5: Breaking Existing Tests Without Updating
**What goes wrong:** Existing sqllogictest files (phase31_fan_trap.test, phase32_role_playing.test) use explicit cardinality keywords that will no longer parse.
**Why it happens:** The old syntax is removed, but tests are not updated.
**How to avoid:** Update all existing sqllogictest files to use the new syntax (no cardinality keywords, add UNIQUE constraints where OneToOne was intended). Create a new phase33 test file for the new features.
**Warning signs:** `just test-all` fails immediately on existing tests.

### Pitfall 6: DESCRIBE Output Schema Change
**What goes wrong:** Adding new columns to DESCRIBE breaks the output schema and existing tests.
**Why it happens:** `DescribeSemanticViewVTab` has a fixed 8-column schema.
**How to avoid:** Rather than adding new columns, enhance the existing `joins` column JSON to include inferred cardinality, and enhance the tables section to include UNIQUE constraints. The JSON content changes, not the column schema.
**Warning signs:** DESCRIBE queries fail or return wrong column count.

## Code Examples

### UNIQUE Constraint Parsing (body_parser.rs)

```rust
// Find "UNIQUE" keyword with word-boundary matching (reuses find_keyword_ci pattern)
fn find_unique(upper_text: &str) -> Option<(usize, usize)> {
    let bytes = upper_text.as_bytes();
    let keyword = b"UNIQUE";
    let kw_len = keyword.len();
    let mut i = 0;
    while i + kw_len <= bytes.len() {
        if upper_text[i..i + kw_len].as_bytes() == keyword {
            let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let after_ok = i + kw_len == bytes.len() || !bytes[i + kw_len].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return Some((i, i + kw_len));
            }
        }
        i += 1;
    }
    None
}

// In parse_single_table_entry, after PRIMARY KEY parsing:
// Collect UNIQUE constraints
let mut unique_constraints = Vec::new();
let mut remaining = &after_pk[close_pk_paren_pos..]; // text after PK(...)
loop {
    let upper_remaining = remaining.to_ascii_uppercase();
    if let Some((u_start, u_end)) = find_unique(&upper_remaining) {
        let after_unique = remaining[u_end..].trim_start();
        if let Some(cols_str) = extract_paren_content(after_unique) {
            let cols: Vec<String> = cols_str
                .split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect();
            unique_constraints.push(cols);
            // Advance past the closing paren
            let close = after_unique.find(')').unwrap();
            remaining = &after_unique[close + 1..];
        } else {
            break; // No paren after UNIQUE
        }
    } else {
        break; // No more UNIQUE keywords
    }
}
```

### Cardinality Inference (parse.rs)

```rust
/// Infer cardinality for each relationship based on PK/UNIQUE constraints.
/// Also resolves ref_columns (the columns on the target side).
fn infer_cardinality(
    tables: &[TableRef],
    relationships: &mut [Join],
) -> Result<(), ParseError> {
    for join in relationships.iter_mut() {
        if join.fk_columns.is_empty() {
            continue;
        }

        let to_alias_lower = join.table.to_ascii_lowercase();
        let from_alias_lower = join.from_alias.to_ascii_lowercase();

        // Find target table
        let target = tables.iter()
            .find(|t| t.alias.to_ascii_lowercase() == to_alias_lower);
        // Find source table
        let source = tables.iter()
            .find(|t| t.alias.to_ascii_lowercase() == from_alias_lower);

        // Resolve ref_columns: if ref_columns already set from REFERENCES target(cols),
        // validate it matches a PK or UNIQUE on target.
        // If ref_columns is empty, use target's PK.
        if join.ref_columns.is_empty() {
            // REFERENCES target (no column list) -> use PK
            if let Some(target) = target {
                if target.pk_columns.is_empty() {
                    let rel_name = join.name.as_deref().unwrap_or("?");
                    return Err(ParseError {
                        message: format!(
                            "Table '{}' has no PRIMARY KEY. \
                             Specify referenced columns explicitly: \
                             REFERENCES {}(col).",
                            target.alias, target.alias
                        ),
                        position: None,
                    });
                }
                join.ref_columns = target.pk_columns.clone();
            }
        }

        // CARD-03: Validate ref_columns match a PK or UNIQUE on target
        // (validation details in graph.rs)

        // CARD-04: Infer cardinality from FK side
        if let Some(source) = source {
            let fk_set: HashSet<String> = join.fk_columns.iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            let pk_set: HashSet<String> = source.pk_columns.iter()
                .map(|c| c.to_ascii_lowercase())
                .collect();

            if fk_set == pk_set {
                join.cardinality = Cardinality::OneToOne;
            } else {
                // Check UNIQUE constraints
                let matches_unique = source.unique_constraints.iter().any(|uc| {
                    let uc_set: HashSet<String> = uc.iter()
                        .map(|c| c.to_ascii_lowercase())
                        .collect();
                    fk_set == uc_set
                });
                join.cardinality = if matches_unique {
                    Cardinality::OneToOne
                } else {
                    Cardinality::ManyToOne
                };
            }
        }
    }
    Ok(())
}
```

### Fan Trap Adaptation (expand.rs)

```rust
// With OneToMany removed, fan trap detection simplifies:
// ManyToOne forward = safe, ManyToOne reverse = fan-out
// OneToOne = always safe in both directions

// check_path_up: walking from node toward root
if let Some((card, rel_name)) = card_map.get(&(current.clone(), parent.clone())) {
    // Edge: current -> parent (forward direction)
    // ManyToOne forward = safe, OneToOne = safe
    // No OneToMany variant exists, so no fan-out check needed here
} else if let Some((card, rel_name)) = card_map.get(&(parent.clone(), current.clone())) {
    // Edge: parent -> current (we traverse in reverse: current -> parent)
    // ManyToOne reverse = fan-out!
    if *card == Cardinality::ManyToOne {
        return Some(ExpandError::FanTrap { ... });
    }
    // OneToOne reverse = safe
}
```

### Old-JSON Detection (define.rs)

```rust
// After from_json succeeds but before validate_graph
for join in &def.joins {
    if !join.fk_columns.is_empty() && join.ref_columns.is_empty() {
        return Err(Box::<dyn std::error::Error>::from(
            "This semantic view was created with an older version. \
             Please recreate it with the new DDL syntax."
        ));
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Explicit cardinality keywords in DDL | Inferred from PK/UNIQUE constraints | Phase 33 (v0.5.4) | Simpler DDL, Snowflake-aligned, breaking change |
| Three-variant Cardinality enum | Two-variant (ManyToOne, OneToOne) | Phase 33 (v0.5.4) | Simpler fan trap logic, clearer semantics |
| FK columns zipped with target PK | FK columns zipped with ref_columns (PK or UNIQUE) | Phase 33 (v0.5.4) | Supports UNIQUE-referenced relationships |
| Backward-compatible serde defaults | Reject old JSON with clear error | Phase 33 (v0.5.4) | Clean break, no legacy baggage |

**Deprecated/outdated:**
- `parse_cardinality_tokens` function: deleted entirely
- `Cardinality::OneToMany` variant: deleted
- Explicit cardinality keywords in DDL: removed from parser

## Open Questions

1. **UNIQUE constraint on the same columns as PK**
   - What we know: Snowflake docs say "If you already identified a column as a primary key column, do not add the UNIQUE clause for that column."
   - What's unclear: Should we enforce this restriction?
   - Recommendation: Yes, emit a define-time warning or error. It is redundant and could cause confusion in inference. However, if this complicates implementation significantly, skip the check -- the inference logic handles it correctly either way (PK match is checked first).

2. **TABLES clause without PRIMARY KEY for fact tables**
   - What we know: User decision says "Tables without PK or UNIQUE can exist (e.g., fact tables) but cannot be REFERENCES targets."
   - What's unclear: Currently, PRIMARY KEY is REQUIRED in the parser for every table entry. Should we make it optional?
   - Recommendation: Make PRIMARY KEY optional in `parse_single_table_entry`. A table entry can be: `alias AS table_name [PRIMARY KEY (cols)] [UNIQUE (cols) ...]`. Fact tables (leaf nodes that only have outgoing FK references, never incoming) don't need PK/UNIQUE. This is a small but important usability improvement aligned with the Snowflake model.

3. **Order of checks for REFERENCES target(cols) matching CARD-03**
   - What we know: User wants error message to show available constraints.
   - Recommendation: Check PK first, then UNIQUE constraints in declaration order. If none match, list available constraints in the error: "Available: PK(id), UNIQUE(email), UNIQUE(code, region)."

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust test + sqllogictest + Python integration |
| Config file | `Cargo.toml` (test config), `Makefile` (sqllogictest runner) |
| Quick run command | `cargo test` |
| Full suite command | `just test-all` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CARD-01 | UNIQUE constraint parsing in TABLES clause | unit | `cargo test body_parser -- unique` | No -- Wave 0 |
| CARD-01 | UNIQUE constraint stored in model | unit | `cargo test model -- unique` | No -- Wave 0 |
| CARD-02 | Multiple UNIQUE constraints on one table | unit | `cargo test body_parser -- multiple_unique` | No -- Wave 0 |
| CARD-03 | FK must reference PK/UNIQUE on target | unit + sql | `cargo test graph -- fk_ref_validation` | No -- Wave 0 |
| CARD-04 | Cardinality inferred from FK-side constraints | unit | `cargo test parse -- infer_cardinality` | No -- Wave 0 |
| CARD-05 | Old cardinality keywords rejected by parser | unit + sql | `cargo test body_parser -- no_cardinality_keywords` | No -- Wave 0 |
| CARD-06 | ManyToMany/OneToMany removed from enum | unit | `cargo test model -- cardinality_enum` | No -- Wave 0 |
| CARD-07 | REFERENCES target vs REFERENCES target(cols) | unit + sql | `cargo test body_parser -- references_column_list` | No -- Wave 0 |
| CARD-08 | Fan trap detection with inferred cardinality | sql | `just test-sql` (phase33 test file) | No -- Wave 0 |
| CARD-09 | Composite FK subset rejection | unit | `cargo test graph -- composite_fk_subset` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase33_cardinality_inference.test` -- end-to-end sqllogictest for CARD-01 through CARD-09
- [ ] Update `test/sql/phase31_fan_trap.test` -- remove cardinality keywords, use new syntax
- [ ] Update `test/sql/phase32_role_playing.test` -- remove cardinality keywords, add UNIQUE where needed
- [ ] Update `test/sql/phase26_join_resolution.test` -- if it uses cardinality keywords
- [ ] Unit tests in `src/model.rs` -- Cardinality two-variant enum, TableRef with unique_constraints, Join with ref_columns
- [ ] Unit tests in `src/body_parser.rs` -- UNIQUE parsing, REFERENCES(cols) parsing, no cardinality tokens
- [ ] Unit tests in `src/graph.rs` -- CARD-03/09 validation functions
- [ ] Unit tests in `src/parse.rs` -- cardinality inference function
- [ ] Property-based tests in `tests/expand_proptest.rs` and `tests/parse_proptest.rs` -- may need updates for new model fields

## Sources

### Primary (HIGH confidence)
- Codebase analysis: `src/model.rs`, `src/body_parser.rs`, `src/graph.rs`, `src/expand.rs`, `src/parse.rs`, `src/ddl/define.rs`, `src/ddl/describe.rs` -- direct code reading, all patterns verified
- `CONTEXT.md` (33-CONTEXT.md) -- user decisions from discussion phase
- `REQUIREMENTS.md` -- CARD-01 through CARD-09

### Secondary (MEDIUM confidence)
- Snowflake CREATE SEMANTIC VIEW docs (https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- confirmed UNIQUE constraint syntax, REFERENCES syntax, PK/UNIQUE requirement for referenced columns. Cardinality inference details not explicitly documented by Snowflake (our inference model is project-specific).

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, all existing Rust patterns
- Architecture: HIGH -- clear dependency chain through 5 files, well-understood parsing patterns
- Pitfalls: HIGH -- identified from direct code analysis, specific line references
- Validation: HIGH -- existing test infrastructure, clear test map

**Research date:** 2026-03-15
**Valid until:** 2026-04-15 (stable codebase, no external dependencies changing)
