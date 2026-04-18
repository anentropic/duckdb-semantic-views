# Phase 51: YAML Parser Core - Research

**Researched:** 2026-04-18
**Domain:** YAML deserialization, serde integration, Rust model reuse
**Confidence:** HIGH

## Summary

Phase 51 adds a `from_yaml` function to `SemanticViewDefinition` that deserializes YAML strings into the same Rust structs that SQL DDL and JSON already populate. Because all model types already derive `serde::Deserialize` with no rename attributes and no `deny_unknown_fields`, the YAML deserializer can reuse the exact same serde machinery -- the only new code is a thin `from_yaml` method, a size cap check, and thorough tests proving equivalence with JSON.

The user has selected `yaml_serde` (crate name on crates.io; GitHub: `yaml/yaml-serde`) v0.10.4 as the YAML library. This is the official YAML org's maintained fork of the archived `serde_yaml`. It is MIT OR Apache-2.0 licensed (compatible with deny.toml), has no RUSTSEC advisories, and uses `libyaml-rs` (pure Rust bindings to libyaml, MIT licensed) as its backend.

Define-time validation (graph validation, expression checks, DAG resolution) runs at bind time in `DefineFromJsonVTab::bind()` on the deserialized `SemanticViewDefinition`. Since Phase 51 produces the same `SemanticViewDefinition` struct, no validation extraction is needed -- the YAML path will naturally pass through all existing validation when integrated in Phase 52.

**Primary recommendation:** Add `yaml_serde = "0.10"` to Cargo.toml, implement `SemanticViewDefinition::from_yaml(name, yaml_str)` in model.rs (mirroring `from_json`), add a `from_yaml_with_size_cap` wrapper enforcing 1MB, and write comprehensive tests proving YAML/JSON equivalence across all struct variants.

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass (Rust unit tests + proptests + sqllogictest + DuckLake CI)
- **Test coverage:** Every phase must include unit tests, proptests, sqllogictest, and consider fuzz targets
- **Build:** `cargo test` runs without the extension feature (in-memory DuckDB)
- **SQL logic tests:** Require `just build` first; cover integration paths Rust tests miss
- **Linting:** clippy pedantic + fmt + cargo-deny before pushing to main
- **Fuzz targets:** Compilation checked via `just check-fuzz` (nightly)

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| YAML-03 | YAML schema supports all SemanticViewDefinition fields: tables, relationships, dimensions, metrics, facts, and metadata annotations | All model structs derive `Serialize, Deserialize` with serde defaults -- yaml_serde's `from_str` will deserialize all fields identically to serde_json. No rename attributes exist; field names in YAML match Rust struct names. |
| YAML-05 | YAML and SQL DDL produce identical internal representations -- same validation, persistence, and query behavior | `from_yaml` returns `SemanticViewDefinition` -- the same type that `from_json` returns. Validation runs on the struct at bind time. Equivalence proven by test: serialize a struct to JSON and YAML, deserialize both, assert equality. |
| YAML-09 | YAML input is size-capped to prevent anchor/alias bomb denial-of-service | Pre-parse byte length check: `if yaml_str.len() > 1_048_576 { return Err(...) }`. Applied before calling `yaml_serde::from_str`. Per decision #2, this is a sanity guard, not a security boundary. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| yaml_serde | 0.10.4 | YAML deserialization/serialization | Official YAML org fork of archived serde_yaml; MIT OR Apache-2.0; uses libyaml-rs backend; no RUSTSEC advisories; API-compatible with serde_yaml [VERIFIED: crates.io, docs.rs, GitHub yaml/yaml-serde] |
| serde | 1 (existing) | Derive Serialize/Deserialize | Already in Cargo.toml [VERIFIED: Cargo.toml line 34] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde_json | 1 (existing) | JSON round-trip in equivalence tests | Already in Cargo.toml; used in tests to prove YAML/JSON produce identical structs [VERIFIED: Cargo.toml line 35] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| yaml_serde | serde_yaml_ng 0.10 | serde_yaml_ng uses unsafe-libyaml (archived dtolnay crate); yaml_serde uses libyaml-rs; both are API-compatible forks of serde_yaml. User decision: yaml_serde. |
| yaml_serde | serde_yml | Has RUSTSEC-2025-0068 advisory (unsound + unmaintained). Excluded. |
| yaml_serde | serde-saphyr | Different API (not serde_yaml API-compatible). Would require more code changes. |

**Installation:**
```bash
cargo add yaml_serde@0.10
```

Or add to Cargo.toml:
```toml
yaml_serde = "0.10"
```

**Cargo rename option** (for import compatibility with serde_yaml docs/examples):
```toml
serde_yaml = { package = "yaml_serde", version = "0.10" }
```
This lets code use `serde_yaml::from_str` etc. However, since this is a new crate in the project, using the native name `yaml_serde::from_str` is clearer and avoids confusion.

**Version verification:** v0.10.4 is the latest version on crates.io. [VERIFIED: crates.io/crates/yaml_serde, docs.rs/yaml_serde/0.10.4]

**License compatibility:** yaml_serde is MIT OR Apache-2.0. Its dependency libyaml-rs is MIT. Both are in deny.toml's allow list. [VERIFIED: deny.toml, docs.rs/crate/yaml_serde/latest/source/Cargo.toml]

**RUSTSEC status:** No advisories for yaml_serde or libyaml-rs. [VERIFIED: rustsec.org search, April 2026]

**MSRV compatibility:** yaml_serde requires Rust 1.82+. Project pins Rust 1.95.0 in rust-toolchain.toml. Compatible. [VERIFIED: rust-toolchain.toml, docs.rs/yaml_serde/0.10.4]

**Note on STATE.md discrepancy:** STATE.md records "serde_yaml_ng 0.10 selected" from roadmap creation. The user's subsequent assumptions discussion (Phase 51 decisions, key decision #1) overrides this to `yaml-serde` (crates.io name: `yaml_serde`). The user's most recent decision takes precedence.

## Architecture Patterns

### Recommended Project Structure
```
src/
  model.rs          # Add from_yaml + from_yaml_with_size_cap methods
  lib.rs            # No changes needed (model is already public)
fuzz/
  fuzz_targets/
    fuzz_yaml_parse.rs  # New fuzz target (mirrors fuzz_json_parse.rs)
  Cargo.toml            # Add yaml_serde dependency + new [[bin]] entry
```

### Pattern 1: Mirror the `from_json` Pattern
**What:** Add `from_yaml` and `from_yaml_with_size_cap` as associated functions on `SemanticViewDefinition`, mirroring the existing `from_json` method.
**When to use:** This is the only pattern needed for Phase 51.
**Example:**
```rust
// Source: existing from_json pattern in src/model.rs lines 420-425
impl SemanticViewDefinition {
    /// Maximum YAML input size (1 MiB). Sanity guard against oversized input.
    /// This is NOT a security boundary -- creating semantic views is a
    /// privileged operation. See decision docs for trust assumption.
    const YAML_SIZE_CAP: usize = 1_048_576;

    /// Parse and validate a YAML string, returning a typed definition.
    ///
    /// Returns an error if the YAML is invalid or missing required fields.
    /// The `name` parameter is used only in the error message for context.
    pub fn from_yaml(name: &str, yaml: &str) -> Result<Self, String> {
        let def: Self = yaml_serde::from_str(yaml)
            .map_err(|e| format!("invalid YAML definition for semantic view '{name}': {e}"))?;
        Ok(def)
    }

    /// Parse YAML with a size cap check.
    ///
    /// Rejects input exceeding `YAML_SIZE_CAP` (1 MiB) before parsing.
    /// Returns an error with a clear message including the actual size.
    pub fn from_yaml_with_size_cap(name: &str, yaml: &str) -> Result<Self, String> {
        if yaml.len() > Self::YAML_SIZE_CAP {
            return Err(format!(
                "YAML definition for semantic view '{name}' exceeds size limit \
                 ({} bytes > {} byte cap)",
                yaml.len(),
                Self::YAML_SIZE_CAP,
            ));
        }
        Self::from_yaml(name, yaml)
    }
}
```

### Pattern 2: YAML-JSON Equivalence Test
**What:** For each test case, construct a `SemanticViewDefinition` programmatically, serialize to both JSON and YAML, deserialize both, and assert the resulting structs are identical.
**When to use:** Core correctness tests for YAML-03 and YAML-05.
**Example:**
```rust
// Test strategy: build struct -> serialize to JSON + YAML -> deserialize both -> assert eq
fn assert_yaml_json_equivalent(def: &SemanticViewDefinition) {
    let json_str = serde_json::to_string(def).unwrap();
    let yaml_str = yaml_serde::to_string(def).unwrap();

    let from_json = SemanticViewDefinition::from_json("test", &json_str).unwrap();
    let from_yaml = SemanticViewDefinition::from_yaml("test", &yaml_str).unwrap();

    // With PartialEq derived:
    assert_eq!(from_json, from_yaml);
}
```

### Pattern 3: PartialEq Derive for Test Assertions
**What:** Add `PartialEq` derive to model structs to simplify equivalence assertions.
**When to use:** Makes tests much cleaner. All model structs are value types -- PartialEq is semantically correct.
**Example:**
```rust
// Add PartialEq to all model structs that need equivalence testing:
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticViewDefinition { ... }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableRef { ... }
// ... etc for Dimension, Metric, Fact, Join, JoinColumn, NonAdditiveDim,
//     WindowSpec, WindowOrderBy
// SortOrder, NullsOrder, Cardinality, AccessModifier already derive PartialEq
```

### Anti-Patterns to Avoid
- **Separate YAML model types:** Do NOT create separate struct types for YAML deserialization. The existing model types with serde derives work directly. Creating parallel types would be maintenance burden and defeat the purpose of YAML-05 (identical representations).
- **Custom deserializers:** Do NOT write custom `Deserialize` impls for YAML. The derived `Deserialize` impls already handle all field defaulting correctly via `#[serde(default)]` attributes. yaml_serde respects these identically to serde_json.
- **Validation in from_yaml:** Do NOT add validation logic to `from_yaml`. Validation belongs in `DefineFromJsonVTab::bind()` (Phase 52 integration). Phase 51 is library-level parsing only.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| YAML parsing | Custom YAML tokenizer/parser | yaml_serde::from_str | YAML spec is complex (anchors, aliases, multiline strings, flow/block styles); libyaml handles edge cases |
| Struct deserialization | Manual field extraction from YAML tree | serde Deserialize derive | Already working on all 12+ model structs; handles defaults, optionals, skip_serializing_if |
| Size cap enforcement | Streaming parser with byte counting | Simple `yaml.len()` check before `from_str` | Per decision #2, this is a sanity guard; byte length of the input string is sufficient |

**Key insight:** The entire implementation is < 20 lines of new code (two functions) because serde + yaml_serde do all the work. The test suite is the substantial part of this phase.

## Common Pitfalls

### Pitfall 1: YAML Boolean Coercion
**What goes wrong:** YAML 1.1 treats `yes`, `no`, `on`, `off`, `true`, `false` as boolean values. A dimension name like `on` or a string value `yes` could be deserialized as `true` instead of the string `"on"` or `"yes"`.
**Why it happens:** libyaml implements YAML 1.1 which has aggressive boolean interpretation. serde_yaml (and forks) preserve this behavior.
**How to avoid:** Document in YAML examples that string values which match YAML boolean literals must be quoted: `name: "on"`, `name: "yes"`. In practice, semantic view field values are SQL expressions and dimension/metric names, which rarely collide with YAML booleans.
**Warning signs:** Tests with field values like `on`, `off`, `yes`, `no`, `true`, `false` should be included to verify behavior. [ASSUMED -- yaml_serde behavior inherited from serde_yaml which used YAML 1.1 booleans]

### Pitfall 2: YAML Tag/Anchor Complexity
**What goes wrong:** YAML anchors (`&anchor`) and aliases (`*anchor`) can create deeply nested or exponentially expanding documents (billion laughs attack). However, per decision #2, this is NOT a priority -- creating a semantic view is a privileged operation guarded by warehouse auth.
**Why it happens:** YAML spec allows anchors/aliases by default.
**How to avoid:** The 1MB size cap is the only guard. Document the trust assumption: "YAML input is trusted (privileged operation). The size cap is a sanity guard, not a security boundary."
**Warning signs:** N/A -- explicitly accepted risk per user decision.

### Pitfall 3: Enum Variant Serialization Format
**What goes wrong:** serde can serialize Rust enums as different YAML formats depending on the enum type. `AccessModifier::Private` could serialize as `Private` (string) or as `access: Private` (mapping) or other formats.
**Why it happens:** serde enum serialization is configurable. The existing model uses the default (externally tagged) representation which serializes unit-like variants as bare strings.
**How to avoid:** Verify with explicit tests that enum values (`AccessModifier`, `SortOrder`, `NullsOrder`, `Cardinality`) serialize/deserialize identically in YAML and JSON. All four enums are simple C-style enums -- serde serializes them as strings (e.g., `"Public"`, `"Private"`, `"Asc"`, `"Desc"`, `"ManyToOne"`, `"OneToOne"`, `"Last"`, `"First"`).
**Warning signs:** Roundtrip tests for each enum variant.

### Pitfall 4: Empty Vec vs Missing Field in YAML
**What goes wrong:** In YAML, omitting a field vs providing `field: []` could behave differently.
**Why it happens:** The model uses `#[serde(default)]` on Vec fields, so omitting a field defaults to empty Vec. Providing `field: []` also produces empty Vec. Both should work identically.
**How to avoid:** Test both: (a) YAML with field omitted, (b) YAML with field as empty list. Already covered by existing `#[serde(default)]` attributes on all optional/Vec fields.
**Warning signs:** Deserialization errors on minimal YAML input (only required fields).

### Pitfall 5: Cargo Feature Interaction
**What goes wrong:** yaml_serde needs to be available under both the default feature (bundled, for `cargo test`) AND the `extension` feature (cdylib build). If it's feature-gated incorrectly, `cargo test` or extension builds could fail.
**Why it happens:** The project has a `default`/`extension` feature split. yaml_serde has no feature-gate requirement -- it's a pure serde library.
**How to avoid:** Add `yaml_serde` as an unconditional dependency in `[dependencies]` (no features gating). Same as serde_json.
**Warning signs:** `cargo test` passes but `just build` (extension mode) fails, or vice versa.

### Pitfall 6: YAML `joins` Key vs Model Field Name
**What goes wrong:** In the `SemanticViewDefinition` struct, the field is named `joins` but the SQL DDL clause is called `RELATIONSHIPS`. In YAML, the key must be `joins` (matching the Rust field name) -- NOT `relationships`.
**Why it happens:** The model was designed before the SQL body parser was added. The body parser maps `RELATIONSHIPS` clause -> `joins` field. YAML deserialization uses the struct field names directly.
**How to avoid:** Document clearly in YAML schema examples that the key is `joins`, not `relationships`. Consider whether to add `#[serde(alias = "relationships")]` for user ergonomics (but note: no aliases exist currently, so adding one would be a precedent). Decision: use `joins` for now, matching existing JSON persistence format. Phase 56 (YAML export) can revisit.
**Warning signs:** Users writing `relationships:` in YAML instead of `joins:` getting deserialization errors.

## Code Examples

### Minimal YAML Input
```yaml
# Source: derived from model.rs SemanticViewDefinition required fields
base_table: orders
dimensions:
  - name: region
    expr: region
metrics:
  - name: revenue
    expr: SUM(amount)
```

### Full YAML Input (All Fields)
```yaml
# Source: derived from model.rs SemanticViewDefinition, all fields populated
base_table: orders
tables:
  - alias: o
    table: orders
    pk_columns:
      - id
    unique_constraints:
      - - email
      - - first_name
        - last_name
    comment: Main orders table
    synonyms:
      - order_facts
  - alias: c
    table: customers
    pk_columns:
      - id
joins:
  - table: c
    from_alias: o
    fk_columns:
      - customer_id
    ref_columns:
      - id
    name: order_to_customer
    cardinality: ManyToOne
facts:
  - name: unit_price
    expr: o.price / o.qty
    source_table: o
    output_type: DOUBLE
    comment: Price per unit
    synonyms:
      - price_per_item
    access: Private
dimensions:
  - name: region
    expr: o.region
    source_table: o
    output_type: VARCHAR
    comment: Geographic region
    synonyms:
      - area
      - territory
metrics:
  - name: revenue
    expr: SUM(o.amount)
    source_table: o
    output_type: DECIMAL(18,2)
    comment: Total revenue
    synonyms:
      - total_revenue
    access: Public
    using_relationships:
      - order_to_customer
  - name: balance
    expr: SUM(o.amount)
    source_table: o
    non_additive_by:
      - dimension: date_dim
        order: Desc
        nulls: First
  - name: avg_qty_7d
    expr: AVG(total_qty)
    window_spec:
      window_function: AVG
      inner_metric: total_qty
      excluding_dims:
        - date_dim
      order_by:
        - expr: date_dim
          order: Asc
          nulls: Last
      frame_clause: "RANGE BETWEEN INTERVAL '6 days' PRECEDING AND CURRENT ROW"
comment: Revenue analytics view
```

### YAML Deserialization (from_yaml function)
```rust
// Source: pattern from model.rs from_json (lines 420-425)
pub fn from_yaml(name: &str, yaml: &str) -> Result<Self, String> {
    let def: Self = yaml_serde::from_str(yaml)
        .map_err(|e| format!("invalid YAML definition for semantic view '{name}': {e}"))?;
    Ok(def)
}
```

### YAML Size Cap
```rust
// Source: YAML-09 requirement + decision #2
pub fn from_yaml_with_size_cap(name: &str, yaml: &str) -> Result<Self, String> {
    if yaml.len() > Self::YAML_SIZE_CAP {
        return Err(format!(
            "YAML definition for semantic view '{name}' exceeds size limit \
             ({} bytes > {} byte cap)",
            yaml.len(),
            Self::YAML_SIZE_CAP,
        ));
    }
    Self::from_yaml(name, yaml)
}
```

### Fuzz Target
```rust
// Source: pattern from fuzz/fuzz_targets/fuzz_json_parse.rs
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Must not panic regardless of input.
        // Errors are fine -- panics/UB are not.
        let _ = semantic_views::model::SemanticViewDefinition::from_yaml("fuzz_test", s);
    }
});
```

### YAML-JSON Equivalence Proptest
```rust
// Source: pattern from tests/expand_proptest.rs + arbitrary feature
use proptest::prelude::*;
use semantic_views::model::SemanticViewDefinition;

proptest! {
    #[test]
    fn yaml_json_roundtrip_equivalence(def in any::<SemanticViewDefinition>()) {
        // Serialize to JSON and YAML
        let json_str = serde_json::to_string(&def).unwrap();
        let yaml_str = yaml_serde::to_string(&def).unwrap();

        // Deserialize both
        let from_json = SemanticViewDefinition::from_json("test", &json_str).unwrap();
        let from_yaml = SemanticViewDefinition::from_yaml("test", &yaml_str).unwrap();

        // Assert structural equality
        prop_assert_eq!(from_json, from_yaml);
    }
}
```

### Note on YAML Field Naming
```yaml
# YAML field names match Rust struct field names exactly (snake_case).
# No #[serde(rename)] attributes exist on any model struct.
# Verified: grep for serde(rename in src/model.rs returns no matches.
#
# YAML keys:
#   base_table, tables, dimensions, metrics, joins, facts,
#   column_type_names, column_types_inferred, created_on,
#   database_name, schema_name, comment
#   (on TableRef) alias, table, pk_columns, unique_constraints, synonyms
#   (on Dimension) name, expr, source_table, output_type, comment, synonyms
#   (on Metric) name, expr, source_table, output_type, using_relationships,
#               comment, synonyms, access, non_additive_by, window_spec
#   (on Fact) name, expr, source_table, output_type, comment, synonyms, access
#   (on Join) table, on, from_cols, join_columns, from_alias, fk_columns,
#             ref_columns, name, cardinality
#   (on JoinColumn) from, to
#   (on NonAdditiveDim) dimension, order, nulls
#   (on WindowSpec) window_function, inner_metric, extra_args, excluding_dims,
#                   partition_dims, order_by, frame_clause
#   (on WindowOrderBy) expr, order, nulls
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| serde_yaml (dtolnay) | yaml_serde (YAML org fork) | March 2024 (serde_yaml archived) | Drop-in replacement; same API surface |
| serde_yaml v0.9 | yaml_serde v0.10 | 2024-2025 | Minor version bump; Rust edition 2021, MSRV 1.82 |
| unsafe-libyaml backend | libyaml-rs backend | yaml_serde fork | yaml_serde switched from unsafe-libyaml to libyaml-rs |

**Deprecated/outdated:**
- `serde_yaml` (dtolnay): Archived March 2024, no new releases. Do not use.
- `serde_yml`: Has RUSTSEC-2025-0068 (unsound + unmaintained). Do not use.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | yaml_serde's `from_str` handles all `#[serde(default)]` and `#[serde(skip_serializing_if)]` attributes identically to serde_json | Architecture Patterns | If behavior differs, YAML deserialization could fail on optional fields. Mitigated by: extensive equivalence tests will catch this immediately. Risk: LOW. |
| A2 | YAML boolean coercion (YAML 1.1: `yes`/`no`/`on`/`off` as booleans) does not affect typical semantic view definitions | Common Pitfalls | If a dimension/metric name is `on` or `yes`, it would need quoting in YAML. Risk: LOW -- these names are uncommon in analytics schemas. |

**Verified (removed from assumptions):**
- yaml_serde v0.10.4 MSRV (1.82) is compatible with rust-toolchain.toml (pins 1.95.0). [VERIFIED: rust-toolchain.toml]

## Open Questions

1. **PartialEq derive on model structs**
   - What we know: Model structs don't derive `PartialEq`. Enum types (`SortOrder`, `NullsOrder`, `Cardinality`, `AccessModifier`) do.
   - What's unclear: Whether adding `PartialEq` to `SemanticViewDefinition`, `TableRef`, `Dimension`, `Metric`, `Fact`, `Join`, `JoinColumn`, `NonAdditiveDim`, `WindowSpec`, `WindowOrderBy` is acceptable or would trigger clippy warnings (e.g., `f32`/`f64` fields would make PartialEq impure, but these structs only contain `String`, `Vec`, `Option`, `u32`, `bool`, and other PartialEq types).
   - Recommendation: Add PartialEq. All field types are PartialEq-safe (String, Vec, Option, u32, enums). No f32/f64 fields exist. This enables `assert_eq!` and proptest `prop_assert_eq!` in equivalence tests.

2. **Fuzz target for YAML in fuzz/Cargo.toml**
   - What we know: fuzz/Cargo.toml needs a new `[[bin]]` entry for `fuzz_yaml_parse`. The fuzz target only calls `from_yaml` on the main crate.
   - What's unclear: Whether fuzz/Cargo.toml needs a direct yaml_serde dependency or if it comes transitively through `semantic_views`.
   - Recommendation: yaml_serde is a transitive dependency via `semantic_views`. No direct dependency needed in fuzz/Cargo.toml. The fuzz target only calls `semantic_views::model::SemanticViewDefinition::from_yaml`.

3. **YAML key `joins` vs SQL clause `RELATIONSHIPS`**
   - What we know: The Rust field is `joins` but the SQL DDL clause is `RELATIONSHIPS`. YAML uses Rust field names.
   - What's unclear: Whether to add `#[serde(alias = "relationships")]` for user ergonomics.
   - Recommendation: Use `joins` for now (matching JSON persistence and Rust field name). Adding aliases is a decision for the user; document the discrepancy. This avoids setting a precedent for serde aliases that could complicate serialization.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + proptest 1.11 |
| Config file | Cargo.toml (proptest in dev-dependencies) |
| Quick run command | `cargo test model::tests` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| YAML-03 | All SemanticViewDefinition fields deserialize from YAML | unit | `cargo test model::tests::yaml` | Wave 0 |
| YAML-03 | Complex YAML with all field types (enums, nested structs, vecs) roundtrips | unit | `cargo test model::tests::yaml` | Wave 0 |
| YAML-05 | YAML and JSON produce identical SemanticViewDefinition structs | unit + proptest | `cargo test model::tests::yaml` + `cargo test yaml_proptest` | Wave 0 |
| YAML-09 | YAML exceeding 1MB is rejected before parsing | unit | `cargo test model::tests::yaml` | Wave 0 |
| YAML-09 | YAML at exactly 1MB is accepted | unit | `cargo test model::tests::yaml` | Wave 0 |
| YAML-09 | Error message includes actual size and cap | unit | `cargo test model::tests::yaml` | Wave 0 |
| -- | Arbitrary YAML input doesn't panic from_yaml | fuzz | `cargo +nightly fuzz run fuzz_yaml_parse` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] PartialEq derive on model structs (if not already present) for `assert_eq!` in equivalence tests
- [ ] yaml_serde dependency in Cargo.toml
- [ ] `fuzz/fuzz_targets/fuzz_yaml_parse.rs` -- YAML fuzz target
- [ ] `fuzz/Cargo.toml` -- add `[[bin]]` entry for fuzz_yaml_parse

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A (but trust assumption documented: YAML input is privileged) |
| V5 Input Validation | yes | Size cap (1MB) before parsing; serde type enforcement |
| V6 Cryptography | no | N/A |

### Known Threat Patterns for YAML Deserialization

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Billion laughs (anchor/alias bomb) | Denial of Service | 1MB size cap (sanity guard, not security boundary per decision #2) |
| Type confusion via YAML tags | Tampering | serde typed deserialization rejects unexpected types |
| Code execution via YAML tags | Elevation of Privilege | Not applicable -- serde does not execute arbitrary constructors |

## Sources

### Primary (HIGH confidence)
- [Cargo.toml] - existing serde/serde_json dependencies, feature configuration
- [src/model.rs] - full SemanticViewDefinition and all nested struct definitions with serde attributes
- [src/ddl/define.rs] - DefineFromJsonVTab::bind() pipeline showing validation chain
- [deny.toml] - license allow list
- [rust-toolchain.toml] - Rust 1.95.0 pinned
- [docs.rs/yaml_serde/0.10.4] - API surface, Cargo.toml, dependencies
- [github.com/yaml/yaml-serde] - repository status, license, maintenance

### Secondary (MEDIUM confidence)
- [crates.io/crates/yaml_serde] - version 0.10.4, download count (~12K)
- [rustsec.org] - no advisories for yaml_serde or libyaml-rs
- [users.rust-lang.org] - serde_yaml deprecation discussion, community fork landscape

### Tertiary (LOW confidence)
- None -- all claims verified against primary or secondary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - yaml_serde verified on crates.io/docs.rs, license checked against deny.toml, MSRV verified against rust-toolchain.toml
- Architecture: HIGH - existing serde derives on model types confirmed, from_json pattern verified in model.rs, no rename/alias attributes
- Pitfalls: HIGH - YAML boolean coercion is well-documented; enum serialization verified via existing JSON test patterns; joins/relationships naming verified

**Research date:** 2026-04-18
**Valid until:** 2026-05-18 (stable domain; yaml_serde unlikely to change significantly)
