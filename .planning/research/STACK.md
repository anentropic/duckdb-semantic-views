# Technology Stack

**Project:** DuckDB Semantic Views v0.7.0 -- YAML Definitions & Materialization Routing
**Researched:** 2026-04-17

## Recommended Stack Additions

### YAML Parsing: `serde_yaml_ng`

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| serde_yaml_ng | 0.10 | Deserialize/serialize YAML to/from existing `SemanticViewDefinition` | Drop-in serde integration with existing model structs; API mirrors serde_json (`from_str`, `to_string`); maintained fork of dtolnay's serde_yaml; MIT license passes cargo-deny |

**Rationale:** The existing `SemanticViewDefinition` struct and all its nested types (`TableRef`, `Dimension`, `Metric`, `Fact`, `Join`, etc.) already derive `Serialize` and `Deserialize`. Adding YAML support means adding a single dependency and calling `serde_yaml_ng::from_str()` instead of `serde_json::from_str()`. No model changes required.

**Why `serde_yaml_ng` over alternatives:**

| Crate | Version | Status | Verdict |
|-------|---------|--------|---------|
| serde_yaml (dtolnay) | 0.9 | **Archived** March 2024, unmaintained | REJECTED -- abandoned |
| serde_yml | 0.0.x | **RUSTSEC-2025-0068**: unsound, AI-generated nonsense code, archived | REJECTED -- security advisory |
| serde_yaml_bw | 2.5.5 | Maintained fork, supports merge keys | VIABLE but serde-saphyr recommended over it by its own author |
| serde-saphyr | 0.0.23 | No unsafe, panic-free, 1000+ tests, active development | VIABLE but pre-1.0 API (0.0.x), breaking changes expected |
| serde_yaml_ng | 0.10 | Maintained fork of dtolnay's original, MIT, closest API match | **SELECTED** |

**Decision:** `serde_yaml_ng` wins because:
1. **API compatibility** -- identical function signatures to `serde_json` (`from_str`, `to_string`), minimizing learning curve and code patterns
2. **Proven lineage** -- direct fork from dtolnay's high-quality original; minimal divergence from the battle-tested codebase
3. **Version maturity** -- at 0.10, further along than serde-saphyr's 0.0.x; less likely to have breaking API changes
4. **License** -- MIT, already in `deny.toml` allowlist
5. **Dependency weight** -- depends on `serde 1.0` (already in Cargo.toml) and `unsafe-libyaml` (auto-translated C, same backend as the original)

**Risk:** `serde_yaml_ng` currently uses `unsafe-libyaml` (an auto-translated C libyaml binding). The maintainer is actively migrating to `libyaml-safer` (safe Rust port). This is a LOW risk: unsafe-libyaml has been the YAML backend for the entire Rust ecosystem for years; the migration to safe Rust is an improvement, not a fix for known bugs.

**Confidence:** MEDIUM -- verified via GitHub repo, crates.io listing, and RustSec advisories. Not verified via Context7 (not available for this crate).

### No Additional Crates Required

The remaining v0.7.0 features need **zero additional dependencies**:

| Feature | Implementation Approach | Why No New Crate |
|---------|------------------------|------------------|
| Dollar-quoted YAML blocks (`$$ ... $$`) | Extend `body_parser.rs` or `parse.rs` to detect `FROM YAML $$` prefix and extract the YAML body before the closing `$$` | DuckDB already supports `$$` in its SQL parser; our parser hook receives the full query string including dollar-quoted content. Simple string scanning for `$$` delimiters. |
| YAML FILE loading (`FROM YAML FILE '...'`) | `std::fs::read_to_string()` in the DDL handler | Already used in `src/catalog.rs` for migration file I/O. Standard library only. |
| YAML-to-JSON conversion at define time | `serde_yaml_ng::from_str::<SemanticViewDefinition>()` then store as JSON via existing `serde_json::to_string()` | YAML is an input format only; internal storage remains JSON. One-time conversion at define time. |
| GET_DDL YAML export | `serde_yaml_ng::to_string(&def)` in a new `render_yaml.rs` | Direct serialization of existing model struct. May need `#[serde(skip)]` on internal-only fields (column_types_inferred, etc.) or a separate YAML-specific output struct. |
| MATERIALIZATIONS clause | New `Materialization` struct in `model.rs` with `Serialize`/`Deserialize` derives | Pure Rust: new model type, body_parser extension, set-containment matching logic in expansion. |
| Materialization routing | Set-containment matching in `expand/mod.rs` | The algorithm is a simple sequential scan with `is_subset` checks on `HashSet<String>`. Per the design doc, this is a pure function -- no external query planner needed. Already uses `HashSet` from std. |
| Re-aggregation wrapper | SQL string generation wrapping a `FROM materialized_table GROUP BY` | Same pattern as existing CTE-based SQL generation in `expand/sql_gen.rs`. |

## Integration Points with Existing Serde Pipeline

### Current Flow (JSON only)
```
DDL SQL text
  -> body_parser::parse_keyword_body()
  -> SemanticViewDefinition struct
  -> serde_json::to_string()
  -> catalog INSERT (JSON blob in VARCHAR column)

catalog SELECT (JSON blob)
  -> serde_json::from_str::<SemanticViewDefinition>()
  -> expansion engine
```

### New Flow (YAML + JSON)
```
DDL SQL text with FROM YAML $$ ... $$ or FROM YAML FILE '...'
  -> parse.rs: detect YAML prefix, extract YAML body
  -> serde_yaml_ng::from_str::<SemanticViewDefinition>()
  -> serde_json::to_string()                              # convert to JSON for storage
  -> catalog INSERT (JSON blob -- same as before)

GET_DDL('SEMANTIC_VIEW', 'name', 'YAML')                  # optional YAML export
  -> serde_json::from_str::<SemanticViewDefinition>()
  -> serde_yaml_ng::to_string()                           # render as YAML
```

**Key design principle:** YAML is an **input/output format**, not a storage format. Internal catalog persistence remains JSON. This avoids any migration of stored data and preserves backward compatibility with pre-v0.7.0 definitions.

### YAML Field Mapping

The `SemanticViewDefinition` struct's serde attributes (`#[serde(default)]`, `#[serde(skip_serializing_if)]`) work identically for YAML as for JSON. The existing backward-compatibility annotations (empty Vec defaults, None defaults) apply automatically.

Fields to exclude from YAML export (internal-only):
- `column_type_names` / `column_types_inferred` -- DDL-time inference data, not user-facing
- `created_on` / `database_name` / `schema_name` -- runtime metadata, not part of definition

Approach: Either use `#[serde(skip)]` (but this breaks JSON roundtrip) or create a lightweight wrapper/view struct for YAML output that omits these fields.

## Current Stack (Unchanged)

### Core Framework
| Technology | Version | Purpose |
|------------|---------|---------|
| duckdb (crate) | =1.10502.0 | DuckDB Rust bindings, exact-pinned to DuckDB 1.5.2 |
| libduckdb-sys | =1.10502.0 | Raw C API bindings |
| serde | 1 | Serialization/deserialization framework |
| serde_json | 1 | JSON format (catalog persistence) |
| strsim | 0.11 | Levenshtein distance for "did you mean" suggestions |
| cc | 1 (optional) | C++ compilation for extension builds |

### Dev Dependencies
| Technology | Version | Purpose |
|------------|---------|---------|
| proptest | 1.11 | Property-based testing |
| cargo-husky | 1 | Git hooks |

## Installation

```toml
# Add to Cargo.toml [dependencies]:
serde_yaml_ng = "0.10"
```

```bash
cargo add serde_yaml_ng@0.10
```

No `deny.toml` changes needed -- MIT license is already allowed.

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| YAML parsing | serde_yaml_ng 0.10 | serde-saphyr 0.0.23 | Pre-1.0 API with expected breaking changes; we need stability for a single-purpose YAML layer, not a cutting-edge parser. Would be the pick if starting fresh in 2027+. |
| YAML parsing | serde_yaml_ng 0.10 | serde_yml | RUSTSEC-2025-0068 advisory: unsound, AI-generated code, archived. Hard no. |
| YAML parsing | serde_yaml_ng 0.10 | serde_yaml (dtolnay) | Archived March 2024, unmaintained. |
| Materialization routing | Std library (HashSet) | egg (e-graph rewriting) | Massive overkill. Design doc explicitly rules this out: "Our pre-aggregation selector is a pure function. DuckDB handles the rest." Set-containment matching is ~50 lines of Rust. |
| Materialization routing | Std library (HashSet) | Custom query planner | The extension is a preprocessor, not a query engine. DuckDB handles optimization. |
| Dollar-quoted parsing | String scanning in parse.rs | Pest/nom parser combinator | The `$$` delimiter is trivial to detect: find opening `$$`, find closing `$$`, take substring. Adding a parser combinator library for two find operations would be absurd. |
| File I/O | std::fs::read_to_string | DuckDB read_file/read_text | DuckDB's file functions are SQL-level and would require executing SQL to read a file, then passing the result to the YAML parser. Using std::fs is simpler and already established in the codebase (catalog.rs). |

## What NOT to Add

| Temptation | Why Not |
|------------|---------|
| DuckDB YAML community extension | That extension parses YAML values in query results. We need YAML-to-struct deserialization at DDL time. Completely different use case. |
| Full YAML framework (e.g., yaml-rust2 directly) | We need serde integration, not a low-level YAML parser. Our model structs already derive serde traits. |
| Template engine for YAML export | `serde_yaml_ng::to_string()` handles serialization. Manual string building (as in `render_ddl.rs`) is the fallback if the serde output format needs customization. |
| Query planning/rewriting crate | The materialization routing algorithm is 3 checks: (1) are all requested metrics in the materialization? (2) are all requested dimensions in the materialization? (3) are the metrics additive? This is `HashSet::is_subset`. |
| Regex crate | Dollar-quoted string detection needs no regex. `str::find("$$")` suffices. |

## Sources

- [serde_yaml_ng on GitHub](https://github.com/acatton/serde-yaml-ng) -- MIT, maintained fork, v0.10
- [RUSTSEC-2025-0068: serde_yml advisory](https://rustsec.org/advisories/RUSTSEC-2025-0068.html) -- unsound, archived
- [serde-saphyr on GitHub](https://github.com/bourumir-wyngs/serde-saphyr) -- Apache-2.0/MIT, v0.0.23, safe Rust
- [Rust forum: serde_yaml deprecation alternatives](https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868) -- community consensus
- [DuckDB dollar-quoted strings](https://duckdb.org/docs/current/sql/data_types/text) -- native `$$` support
- [DuckDB securing extensions](https://duckdb.org/docs/stable/operations_manual/securing_duckdb/overview) -- file access context
- [Snowflake SYSTEM$CREATE_SEMANTIC_VIEW_FROM_YAML](https://docs.snowflake.com/en/sql-reference/stored-procedures/system_create_semantic_view_from_yaml) -- `$$` YAML syntax reference
- [Design doc: semantic-views-duckdb-design-doc.md](_notes/semantic-views-duckdb-design-doc.md) -- materialization routing algorithm
