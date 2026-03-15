# Architecture: v0.5.4 Snowflake-Parity & Registry Publishing

**Domain:** DuckDB semantic views extension -- UNIQUE constraints, cardinality inference, multi-version DuckDB, CE registry publishing
**Researched:** 2026-03-15
**Confidence:** HIGH (direct codebase analysis, Snowflake official docs, DuckDB extension-ci-tools docs)

## Executive Summary

v0.5.4 introduces four architectural changes: (1) UNIQUE table constraints with Snowflake-style cardinality inference replacing explicit keywords, (2) multi-version DuckDB support for v1.4.4 and v1.5.0, (3) documentation site, and (4) community extension registry publishing. Features 1 and 2 are the only code-affecting changes. Feature 1 modifies three existing subsystems (body_parser, model, graph) and one expansion function (check_fan_traps). Feature 2 requires build system and CI changes, plus potential C++ shim adjustments for DuckDB 1.5.0 ABI. Features 3 and 4 are infrastructure-only.

### Architecture Principle: Expansion-Only (Unchanged)

All code changes remain within the "expansion-only" preprocessor model. UNIQUE constraints and cardinality inference are define-time metadata changes. The expansion pipeline, query table function, FFI layer, and catalog persistence are unaffected.

## Feature 1: UNIQUE Constraints + Cardinality Inference

### Current State

The current TABLES and RELATIONSHIPS syntax:

```sql
TABLES (
  o AS orders PRIMARY KEY (id),
  c AS customers PRIMARY KEY (id)
)
RELATIONSHIPS (
  order_to_customer AS o(customer_id) REFERENCES c MANY TO ONE
)
```

Cardinality is explicitly declared after REFERENCES with `MANY TO ONE`, `ONE TO ONE`, or `ONE TO MANY` keywords. The default when omitted is `ManyToOne` (most common FK pattern).

### Target State (Snowflake-Aligned)

```sql
TABLES (
  o AS orders PRIMARY KEY (id),
  c AS customers PRIMARY KEY (id) UNIQUE (email)
)
RELATIONSHIPS (
  order_to_customer AS o(customer_id) REFERENCES c
)
```

Cardinality is INFERRED from PK/UNIQUE declarations. Explicit keywords are removed. The UNIQUE constraint is a new table-level annotation.

### Component Changes

#### 1. model.rs -- Add `unique_keys` to `TableRef`

**Modification:** Add a new field to the existing `TableRef` struct.

```rust
pub struct TableRef {
    pub alias: String,
    pub table: String,
    pub pk_columns: Vec<String>,
    /// UNIQUE constraint columns for this table.
    /// Multiple UNIQUE constraints are supported (each is a Vec of columns).
    /// Snowflake: "If you already identified a column as a primary key column,
    /// do not add the UNIQUE clause for that column."
    /// Old stored JSON without this field deserializes with empty Vec.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_keys: Vec<Vec<String>>,
}
```

**Rationale for `Vec<Vec<String>>`:** A table may have multiple UNIQUE constraints (e.g., `UNIQUE (email)` and `UNIQUE (phone, country_code)`). Each inner Vec is one constraint covering one or more columns. This matches Snowflake's syntax where multiple UNIQUE clauses are allowed per table.

**Serialization:** `skip_serializing_if = "Vec::is_empty"` ensures backward compatibility -- old definitions without UNIQUE produce identical JSON. Old JSON without the field deserializes to `vec![]` via `#[serde(default)]`.

**Impact on `Cardinality` enum:** The enum itself (`ManyToOne`, `OneToOne`, `OneToMany`) is UNCHANGED. Only the source of the value changes from explicit parse to inference.

#### 2. body_parser.rs -- Parse UNIQUE in TABLES Entries

**Modification to `parse_single_table_entry()`:** Currently parses `alias AS table PRIMARY KEY (cols)`. Must also parse optional `UNIQUE (cols)` clause(s) after PRIMARY KEY.

```
Current: alias AS table PRIMARY KEY (col1, col2)
New:     alias AS table PRIMARY KEY (col1) [UNIQUE (col2, col3)] [UNIQUE (col4)]
```

**Implementation approach:** After extracting `pk_columns` from `PRIMARY KEY (...)`, check if the remaining text contains one or more `UNIQUE (...)` blocks. Parse each UNIQUE block the same way as PRIMARY KEY -- extract parenthesized comma-separated column names.

**Parser changes (specific):**

1. After the PRIMARY KEY `extract_paren_content()` call at line ~465 of body_parser.rs, consume the rest of the entry text.
2. In a loop, find the next occurrence of `UNIQUE` keyword (using `find_keyword_ci` with `"UNIQUE"`).
3. For each `UNIQUE` found, extract the parenthesized content and add to `unique_keys: Vec<Vec<String>>`.
4. If remaining text after PK contains non-whitespace and non-UNIQUE content, emit a parse error.

**Validation at parse time:**
- PK columns must not appear in any UNIQUE constraint (Snowflake rule: "do not add UNIQUE for primary key columns").
- Empty UNIQUE column list is rejected.
- Duplicate UNIQUE constraints (same column set) are rejected.

#### 3. body_parser.rs -- Remove Explicit Cardinality from RELATIONSHIPS

**Modification to `parse_single_relationship_entry()` and `parse_cardinality_tokens()`:**

**Strategy: Deprecation with backward compat.**

Option A (recommended): **Remove** `parse_cardinality_tokens()` entirely. If any tokens remain after `REFERENCES <to_alias>`, emit a parse error with a helpful message: "Explicit cardinality keywords (MANY TO ONE, etc.) are no longer supported. Cardinality is inferred from PRIMARY KEY and UNIQUE constraints in TABLES."

Option B (gradual): Keep `parse_cardinality_tokens()` but emit a deprecation warning. Infer cardinality from PK/UNIQUE anyway and verify it matches explicit declaration.

**Recommendation: Option A.** This is a pre-release extension (v0.x). Breaking changes are expected. Removing the keywords simplifies the syntax and aligns with Snowflake. Old stored JSON with explicit `cardinality` fields still deserializes correctly via serde defaults -- only the DDL surface changes.

#### 4. graph.rs -- Infer Cardinality at Validation Time

**This is the core architectural decision: WHERE does inference happen?**

**Recommendation: Post-parse, in `validate_graph()` (define-time).**

**Rationale:**
- The parser (`body_parser.rs`) should not need to cross-reference TABLES and RELATIONSHIPS. It parses each clause independently.
- Cardinality inference requires knowing BOTH sides of the relationship: the FK-declaring side's constraints AND the referenced side's constraints.
- `validate_graph()` already has access to the full `SemanticViewDefinition` with both `tables` and `joins`.
- This follows the existing pattern: graph validation is a post-parse step in `define.rs bind()`.

**Inference algorithm (new function in graph.rs):**

```rust
/// Infer cardinality for each relationship from PK/UNIQUE constraints.
///
/// Called after parsing, before graph validation. Mutates joins in place.
///
/// Rules (Snowflake-aligned):
/// 1. The REFERENCES target column(s) MUST be covered by a PRIMARY KEY or
///    UNIQUE constraint on the referenced table. If not: error.
/// 2. If FK-declaring side also has PK or UNIQUE covering the FK columns:
///    -> OneToOne (both sides are unique, so the mapping is bijective)
/// 3. Otherwise:
///    -> ManyToOne (many FK rows map to one PK/UNIQUE row)
/// 4. OneToMany is the REVERSE of ManyToOne. Since relationships are always
///    declared from FK side -> PK side, OneToMany only occurs when the
///    FROM side has PK/UNIQUE on the FK columns AND the TO side does NOT.
///    This is unusual (it means the "FK" side is actually the unique side)
///    and would typically indicate the relationship direction is reversed.
///    Treat this as ManyToOne from the declared direction.
///
/// Note: OneToMany as explicitly declared by users in v0.5.3 meant "from
/// the FROM side's perspective, one row maps to many on the TO side."
/// Under inference, this case is detected when the TO side has no PK/UNIQUE
/// covering the referenced columns -- which is an ERROR (references must
/// point to PK or UNIQUE columns).
pub fn infer_cardinality(def: &mut SemanticViewDefinition) -> Result<(), String> {
    // ...
}
```

**Detailed inference logic:**

For each relationship `rel_name AS from_alias(fk_cols) REFERENCES to_alias`:

1. Look up `to_alias` in `def.tables`. Get its `pk_columns` and `unique_keys`.
2. Check if `fk_cols` are "covered" by the referenced table's PK or any UNIQUE:
   - "Covered" means the FK column set is a subset of (or equal to) the PK column set or any UNIQUE column set.
   - If the relationship specifies explicit referenced columns (currently not in syntax but implied by PK), the FK columns map 1:1 to PK columns (already validated by `check_fk_pk_counts()`).
   - If NOT covered: **error** -- "Relationship 'rel_name' references table 'to_alias' but the referenced columns are not covered by PRIMARY KEY or UNIQUE. Add a UNIQUE constraint or adjust the relationship."
3. If covered by PK or UNIQUE on the TO side (mandatory by step 2), check the FROM side:
   - Look up `from_alias` in `def.tables`. Get its `pk_columns` and `unique_keys`.
   - If the FROM side's `fk_cols` are covered by its own PK or any UNIQUE: **OneToOne**.
   - Otherwise: **ManyToOne**.

**Why OneToMany disappears:** In Snowflake's model, relationships always go from FK side (many) to PK/UNIQUE side (one). The "one to many" direction is simply the reverse traversal of a many-to-one edge. The expansion engine already handles this: `check_fan_traps()` checks BOTH directions on the tree path. A `ManyToOne` edge traversed from PK->FK direction IS the "one to many" fan-out case, which is what fan trap detection catches.

**Wait -- what about existing OneToMany usage in v0.5.3?** The v0.5.3 fan trap tests explicitly create `OneToMany` cardinality. Under inference, these cases work as follows:
- If the user declares `o(customer_id) REFERENCES c` where `c` has `PRIMARY KEY (id)`, inference produces `ManyToOne`.
- If the user declares `c(id) REFERENCES o` where `o` does NOT have the referenced columns as PK/UNIQUE, that is an error (references must target PK/UNIQUE).
- The fan trap detection code in `expand.rs` currently checks edge direction. A `ManyToOne` edge from `o->c` means: traversing `o->c` is safe (many go to one), traversing `c->o` is fan-out. This is semantically IDENTICAL to `OneToMany` on a reverse edge. The existing detection logic works correctly because it checks both `(from, to)` and `(to, from)` lookups in `card_map`.

**Specific changes in expand.rs check_fan_traps():**

The `card_map` construction (lines 1014-1029) uses `j.cardinality` directly. After inference, `j.cardinality` will be either `ManyToOne` or `OneToOne` (never `OneToMany`). The fan trap check at lines 1082-1092 calls `check_path_up()` and `check_path_down()`. These already handle bidirectional traversal:
- Walking from child to parent (up): a `ManyToOne` edge is safe in this direction.
- Walking from parent to child (down): a `ManyToOne` edge is FAN-OUT in this direction (one parent maps to many children).

So the existing logic naturally handles the case where `OneToMany` is removed as a variant, because fan-out is detected by DIRECTION of traversal relative to the `ManyToOne` edge. **No changes needed in `check_fan_traps()` itself** -- only the inference step that populates `cardinality` on each `Join`.

**Should `Cardinality::OneToMany` be removed from the enum?**

**Recommendation: YES, remove it.** Under inference, `OneToMany` cannot be produced (it would require the TO side to lack PK/UNIQUE, which is an error). Removing it simplifies the model. However, old stored JSON with `"cardinality": "OneToMany"` must still deserialize. Two options:

- **Option A (recommended):** Keep the enum variant but mark it `#[deprecated]` and have serde deserialize it as `ManyToOne` (with a note that the relationship direction is reversed). Actually, better: keep the three-variant enum for backward compat but never produce `OneToMany` from the inference path. `check_fan_traps()` already handles all three variants correctly.

- **Option B:** Remove the variant, add a custom deserializer that maps `"OneToMany"` to `ManyToOne`. This breaks the semantic meaning of old definitions.

**Final recommendation: Keep `OneToMany` in the enum for serde compatibility. The inference function never produces it. Fan trap detection already handles all three. No code changes needed in expand.rs.**

#### 5. define.rs -- Wire Inference Into Validation Chain

**Modification:** Call `infer_cardinality()` before `validate_graph()`.

```rust
// In DefineFromJsonVTab::bind():

// Step 1: Infer cardinality from PK/UNIQUE constraints (v0.5.4).
crate::graph::infer_cardinality(&mut def)
    .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

// Step 2: Validate relationship graph (existing).
crate::graph::validate_graph(&def)
    .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

// Step 3+: existing validations unchanged...
```

**Important:** `def` must be `mut` for inference to write cardinality values onto `Join` structs. Currently `def` is constructed from `from_json()` and then passed to validations. The mutation happens before any validation, so the flow is clean.

#### 6. Validation: Referenced Columns Must Be PK/UNIQUE

This is a NEW validation that does not exist in v0.5.3. Currently `check_fk_pk_counts()` only verifies that the FK column COUNT matches the PK column COUNT. It does not verify that the referenced table's PK/UNIQUE actually exists.

Under Snowflake-style inference, the referenced side MUST have PK or UNIQUE covering the referenced columns. This validation naturally falls out of the `infer_cardinality()` function (step 2 above produces an error if not covered).

### Data Flow: UNIQUE + Inference

```
DDL Input
  |
  v
body_parser.rs
  |  parse_single_table_entry()  -> TableRef { pk_columns, unique_keys }
  |  parse_single_relationship_entry() -> Join { cardinality: ManyToOne (default) }
  |  (no explicit cardinality keywords parsed)
  |
  v
define.rs bind()
  |
  v
graph::infer_cardinality(&mut def)
  |  For each Join:
  |    1. Check TO side has PK/UNIQUE covering referenced cols -> error if not
  |    2. Check FROM side has PK/UNIQUE covering FK cols -> OneToOne if yes, ManyToOne if no
  |    3. Write inferred cardinality onto Join.cardinality
  |
  v
graph::validate_graph(&def)    (existing, unchanged)
  |  check_fk_pk_counts()      (existing, now also covered by inference)
  |  check_no_diamonds()       (existing, unchanged)
  |  check_no_orphans()        (existing, unchanged)
  |
  v
Stored as JSON               (Join.cardinality serialized as before)
  |
  v
expand.rs                    (reads Join.cardinality, unchanged)
  |  check_fan_traps()        (uses cardinality from Join, unchanged)
  |  resolve_joins_pkfk()     (unchanged)
  |
  v
Final SQL
```

## Feature 2: Multi-Version DuckDB Support

### Current State

- Cargo.toml pins `duckdb = "=1.4.4"` and `libduckdb-sys = "=1.4.4"`
- `.duckdb-version` file contains `v1.4.4`
- Build.yml uses `extension-ci-tools@v1.4.4`
- C++ shim compiled against DuckDB 1.4.4 amalgamation (`cpp/include/duckdb.hpp`)
- DuckDB 1.5.0 released 2026-03-09 (6 days ago)
- duckdb-rs 1.5.0 crate appears to be available

### Multi-Version Strategy

DuckDB extension binaries are **version-pinned** -- a binary built for v1.4.4 cannot load in v1.5.0. The extension-ci-tools repository maintains separate branches per DuckDB version (v1.4.4, v1.5.0). Multi-version support means building and distributing SEPARATE binaries for each supported DuckDB version.

**Recommendation: Git branching, NOT conditional compilation.**

#### Why NOT `#[cfg]` Conditional Compilation

Conditional compilation (`#[cfg(duckdb_version = "1.4")]`) is the wrong abstraction here because:

1. **The Rust code is identical across versions.** The semantic views extension uses the DuckDB C API (via duckdb-rs) and C++ API (via amalgamation). The Rust business logic (parsing, model, graph, expand) does not depend on DuckDB version.

2. **Version differences are in dependencies, not in code paths.** The only things that change between v1.4.4 and v1.5.0 are:
   - `Cargo.toml`: `duckdb = "=1.4.4"` vs `duckdb = "=1.5.0"`
   - `Cargo.toml`: `libduckdb-sys = "=1.4.4"` vs `libduckdb-sys = "=1.5.0"`
   - `.duckdb-version`: `v1.4.4` vs `v1.5.0`
   - `cpp/include/duckdb.hpp` and `cpp/include/duckdb.cpp`: amalgamation files matching the target version
   - Build.yml: `extension-ci-tools@v1.4.4` vs `extension-ci-tools@v1.5.0`

3. **If the C API changes between versions** (e.g., DuckDB 1.5.0 adds a new PEG parser that changes `ParserExtension` behavior), the fix would be in the C++ shim (`shim.cpp`), not in Rust code behind `#[cfg]` flags. And the duckdb-rs/libduckdb-sys crate would handle Rust-side API changes.

4. **The DuckDB community extension ecosystem uses separate branches per version.** This is the established pattern: extension-ci-tools has `v1.4.4` and `v1.5.0` branches. Fighting this pattern adds complexity for no benefit.

#### Recommended Branch Strategy

```
main                  -> latest DuckDB (1.5.0), primary development
  |
  +-- v1.4.x          -> DuckDB 1.4.4 LTS backport branch
```

**Workflow:**
1. All new feature development happens on `main` targeting DuckDB 1.5.0.
2. After each milestone, cherry-pick or merge fixes to `v1.4.x` branch.
3. CI builds both branches. Each produces separate extension binaries.
4. Community extension registry submission includes both versions.

**What lives on the `v1.4.x` branch:**
- Identical Rust source code (same `src/` directory)
- `Cargo.toml` with `duckdb = "=1.4.4"`, `libduckdb-sys = "=1.4.4"`
- DuckDB 1.4.4 amalgamation in `cpp/include/`
- `.duckdb-version` set to `v1.4.4`
- Build.yml targeting `extension-ci-tools@v1.4.4`

### C++ Shim ABI Considerations

The C++ shim (`cpp/src/shim.cpp`) uses these DuckDB C++ types:
- `ParserExtension` (parse_function, plan_function)
- `ParserExtensionParseResult`
- `ParserExtensionParseData`
- `ParserExtensionPlanResult`
- `TableFunction`
- `FunctionData`, `GlobalTableFunctionState`
- `ClientContext`, `TableFunctionBindInput`, `TableFunctionInput`
- `DataChunk`
- `DBConfig`, `DatabaseWrapper`
- `LogicalType::VARCHAR`
- `Value`, `StringValue`

**DuckDB 1.5.0 risk: PEG parser.** DuckDB 1.5.0 ships an experimental PEG parser (disabled by default, opt-in via `CALL enable_peg_parser()`). The traditional parser and `ParserExtension` hooks are **still the default** in 1.5.0. The PEG parser "allows extensions to extend the grammar" but the mechanism may differ from `ParserExtension`.

**Action items:**
1. Build against DuckDB 1.5.0 amalgamation and verify `shim.cpp` compiles without changes.
2. Test that parser hooks work as before (CREATE SEMANTIC VIEW, DROP, DESCRIBE, SHOW).
3. Test with PEG parser enabled (`CALL enable_peg_parser()`) to see if extension hooks still fire.
4. If PEG parser breaks the hooks: document incompatibility, recommend keeping PEG parser disabled.

**Likely outcome:** The 1.5.0 transition should be smooth. The `ParserExtension` API is the same C++ interface. The amalgamation compilation may need the existing Windows patching logic updated (new `#include` patterns), but the macOS/Linux path should be unchanged.

### CI Architecture for Dual-Version Builds

#### Current CI Structure

```
Build.yml
  |
  +-- duckdb-stable-build (v1.4.4)
       uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.4.4
```

#### Proposed CI Structure

```
Build.yml
  |
  +-- duckdb-latest-build (v1.5.0) -- runs on main branch
  |    uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.5.0
  |
  +-- duckdb-lts-build (v1.4.4) -- runs on v1.4.x branch
       uses: duckdb/extension-ci-tools/.github/workflows/_extension_distribution.yml@v1.4.4

DuckDBVersionMonitor.yml
  |
  +-- Now monitors for new DuckDB releases on BOTH branches
  +-- Opens PRs against main (latest) and v1.4.x (LTS)
```

**Alternatively (simpler):** Keep Build.yml on `main` targeting v1.5.0 only. The `v1.4.x` branch has its own Build.yml targeting v1.4.4. Each branch is self-contained. The DuckDB Version Monitor only watches `main`.

**Recommendation: Keep it simple.** Each branch owns its own Build.yml with the appropriate version. No matrix builds across versions. The `v1.4.x` branch is a backport branch with minimal maintenance.

### Cargo.toml Changes for v1.5.0

```toml
[dependencies]
duckdb = { version = "=1.5.0", default-features = false }
libduckdb-sys = "=1.5.0"
```

**Risk:** If `duckdb-rs` 1.5.0 has breaking API changes (renamed methods, changed traits), the Rust code may need updates. Check the `duckdb-rs` changelog before upgrading.

**build.rs changes:** The Windows patching logic in `patch_duckdb_cpp_for_windows()` checks for specific string markers in `duckdb.cpp`. DuckDB 1.5.0 may have different line numbers or patterns. The patching is already defensive (emits a `cargo:warning` if markers are not found), so it degrades gracefully.

## Feature 3: Documentation Site

**Architecture impact: NONE.** This is a static site (likely Zensical on GitHub Pages) generated from markdown. No code changes.

**Build integration:** Add a GitHub Actions workflow that builds the docs site and deploys to GitHub Pages on push to `main`.

## Feature 4: Community Extension Registry Publishing

**Architecture impact: MINIMAL.** Requires:
1. A `description.yml` file in the repo root (already partially exists via the extension template).
2. A PR to `duckdb/community-extensions` repository.
3. Verification that the extension loads correctly via `INSTALL semantic_views FROM community; LOAD semantic_views;`.

**description.yml considerations:**
- `extension.version` is a freeform string (not enforced scheme).
- The build must produce artifacts matching the expected layout (handled by extension-ci-tools).
- Multi-version: may need separate description.yml per DuckDB version, or the CE infrastructure handles version routing.

## Component Boundaries (v0.5.4 Summary)

| Component | Change Type | What Changes | Risk |
|-----------|------------|--------------|------|
| `model.rs` | MODIFY | Add `unique_keys: Vec<Vec<String>>` to `TableRef` | Low -- additive serde field |
| `body_parser.rs` | MODIFY | Parse UNIQUE in TABLES; remove cardinality keywords from RELATIONSHIPS | Medium -- two parser changes |
| `graph.rs` | MODIFY | Add `infer_cardinality()` function | Medium -- new inference logic |
| `define.rs` | MODIFY | Wire `infer_cardinality()` before `validate_graph()` | Low -- one new call |
| `expand.rs` | NONE | Reads `Join.cardinality` as before | None |
| `shim.cpp` | VERIFY | May need recompile against 1.5.0 amalgamation | Low-Medium |
| `build.rs` | VERIFY | Windows patches may need updating for 1.5.0 | Low |
| `Cargo.toml` | MODIFY | Version pin update to 1.5.0 | Low |
| `.duckdb-version` | MODIFY | Update to v1.5.0 | None |
| `Build.yml` | MODIFY | Update extension-ci-tools tag | Low |
| `description.yml` | NEW | CE registry descriptor | None |

## Patterns to Follow

### Pattern 1: Post-Parse Inference (New)

**What:** Compute derived metadata from parsed structures before validation.
**When:** When a property depends on cross-referencing multiple parsed clauses.
**Example:**

```rust
// In define.rs bind(), AFTER parsing, BEFORE validation:
let mut def = SemanticViewDefinition::from_json(&name, &json)?;

// Infer cardinality from PK/UNIQUE (v0.5.4)
graph::infer_cardinality(&mut def)?;

// Validate graph (existing)
graph::validate_graph(&def)?;
```

**Why this pattern:** The parser should not need context from other clauses. Inference is a semantic pass that runs after syntactic parsing is complete. This separates concerns: body_parser.rs handles syntax, graph.rs handles semantics.

### Pattern 2: Backward-Compatible Serde Fields

**What:** Adding new optional fields to serialized structs without breaking old JSON.
**When:** Any model.rs struct change.
**Example:**

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub unique_keys: Vec<Vec<String>>,
```

**Rules:**
- Always use `#[serde(default)]` for new fields.
- Use `skip_serializing_if` to avoid bloating JSON for definitions that don't use the feature.
- Write a test verifying old JSON without the field deserializes correctly.

### Pattern 3: Helpful Error Messages for Syntax Changes

**What:** When removing syntax, provide a clear migration message.
**When:** Breaking DDL changes.
**Example:**

```rust
// If tokens remain after REFERENCES <to_alias>:
if !remaining_tokens.is_empty() {
    return Err(ParseError {
        message: format!(
            "Unexpected tokens after REFERENCES target in relationship '{rel_name}': \
             '{}'. Explicit cardinality (MANY TO ONE, etc.) is no longer supported; \
             cardinality is inferred from PRIMARY KEY and UNIQUE constraints in TABLES.",
            remaining_tokens.join(" ")
        ),
        position: Some(entry_offset),
    });
}
```

## Anti-Patterns to Avoid

### Anti-Pattern 1: Conditional Compilation for DuckDB Versions

**What:** Using `#[cfg(duckdb_14)]` or `#[cfg(duckdb_15)]` to handle version differences.
**Why bad:** The Rust code is version-independent. Differences are in dependencies and amalgamation, not code paths. `#[cfg]` flags create maintenance burden and untestable code paths.
**Instead:** Use separate git branches per DuckDB version. Each branch has its own Cargo.toml and amalgamation files.

### Anti-Pattern 2: Runtime Version Detection

**What:** Checking DuckDB version at runtime and branching behavior.
**Why bad:** Extension binaries are already version-pinned. They can only load in the exact DuckDB version they were compiled for. Runtime detection is redundant.
**Instead:** Rely on compile-time version pinning via Cargo.toml and amalgamation.

### Anti-Pattern 3: Inference During Parsing

**What:** Having `parse_single_relationship_entry()` look up TABLES to infer cardinality.
**Why bad:** Creates coupling between parser and model. The parser operates on text; inference operates on structured data. Mixing them makes both harder to test and maintain.
**Instead:** Parse produces `Join` with default `ManyToOne`. Inference updates it with correct value in a separate pass.

### Anti-Pattern 4: Removing Cardinality from the Serialized Model

**What:** Removing the `cardinality` field from `Join` in the JSON representation.
**Why bad:** Breaks backward compatibility with stored definitions. Forces re-inference every time a definition is loaded.
**Instead:** Keep `cardinality` in JSON. Inference writes the value at define time. At query time, the stored value is read directly with no re-inference needed.

## Suggested Build Order

Build order is driven by dependencies and risk.

### Phase 1: UNIQUE Parsing + Cardinality Inference

**Scope:** model.rs, body_parser.rs, graph.rs, define.rs changes for UNIQUE + inference.
**Dependencies:** None (self-contained).
**Tests:** Unit tests for UNIQUE parsing, inference logic, backward compat serde, fan trap detection with inferred cardinality.

**Sub-steps:**
1. Add `unique_keys` to `TableRef` in model.rs + serde tests.
2. Parse UNIQUE in `parse_single_table_entry()` in body_parser.rs + parser tests.
3. Remove explicit cardinality parsing from `parse_single_relationship_entry()` + error message for old syntax.
4. Implement `infer_cardinality()` in graph.rs + unit tests.
5. Wire inference into define.rs bind chain.
6. Update all existing tests that use explicit cardinality keywords.
7. Update sqllogictest files for new syntax.

### Phase 2: DuckDB 1.5.0 Upgrade

**Scope:** Cargo.toml, .duckdb-version, amalgamation files, Build.yml, potential shim.cpp changes.
**Dependencies:** Phase 1 should be complete (don't mix feature changes with version changes).

**Sub-steps:**
1. Update Cargo.toml to `duckdb = "=1.5.0"`, `libduckdb-sys = "=1.5.0"`.
2. Download DuckDB 1.5.0 amalgamation (`duckdb.hpp`, `duckdb.cpp`).
3. Run `cargo test` -- fix any duckdb-rs API changes.
4. Run `just build` -- verify shim.cpp compiles against 1.5.0 amalgamation.
5. Run `just test-sql` -- verify all sqllogictest pass.
6. Test with PEG parser enabled.
7. Update Build.yml to `extension-ci-tools@v1.5.0`.
8. Create `v1.4.x` backport branch from pre-upgrade commit.

### Phase 3: Documentation Site

**Scope:** GitHub Pages setup, markdown content, deployment workflow.
**Dependencies:** None (can run in parallel with Phase 1 or 2).

### Phase 4: Community Extension Registry Publishing

**Scope:** description.yml, PR to duckdb/community-extensions, LOAD verification.
**Dependencies:** Phase 2 (need builds for current DuckDB version).

**Sub-steps:**
1. Create `description.yml` with correct metadata.
2. Test local `INSTALL` and `LOAD` against built extension binary.
3. Open PR to duckdb/community-extensions.
4. Monitor CI in the community-extensions repo.

## Scalability Considerations

| Concern | Impact | Notes |
|---------|--------|-------|
| UNIQUE constraints per table | Negligible | Linear scan of unique_keys during inference; tables count is small (< 20) |
| Inference pass | O(J * T) where J=joins, T=tables | Both are small; no concern |
| Backport branch maintenance | Low | Only Cargo.toml and amalgamation differ; Rust code is shared |
| PEG parser compatibility | Unknown | DuckDB 1.5.0 PEG parser is experimental and off by default; monitor for future impact |

## Sources

- [Snowflake CREATE SEMANTIC VIEW](https://docs.snowflake.com/en/sql-reference/sql/create-semantic-view) -- UNIQUE constraint syntax, cardinality inference rules (HIGH confidence)
- [Snowflake Semantic View SQL Guide](https://docs.snowflake.com/en/user-guide/views-semantic/sql) -- Relationship definition syntax, PK/UNIQUE requirement for references (HIGH confidence)
- [DuckDB 1.5.0 Announcement](https://duckdb.org/2026/03/09/announcing-duckdb-150) -- PEG parser, experimental status, backward compat (HIGH confidence)
- [DuckDB Extension Versioning](https://duckdb.org/docs/stable/extensions/versioning_of_extensions) -- Version-pinned binaries, no cross-version compat (HIGH confidence)
- [DuckDB Extension CI Tools](https://github.com/duckdb/extension-ci-tools/) -- Branch-per-version strategy, v1.4.4 and v1.5.0 branches (HIGH confidence)
- [DuckDB Community Extensions Development](https://duckdb.org/community_extensions/development) -- description.yml, submission process (HIGH confidence)
- [DuckDB Community Extensions Updating](https://github.com/duckdb/community-extensions/blob/main/UPDATING.md) -- Multi-version support for latest 2 DuckDB versions (HIGH confidence)
- [duckdb-rs crate](https://crates.io/crates/duckdb) -- Rust binding versions, feature flags (HIGH confidence)
