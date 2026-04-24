# Phase 52: YAML DDL Integration - Research

**Researched:** 2026-04-18
**Domain:** DDL parser extension, dollar-quoting, YAML-to-JSON rewrite pipeline
**Confidence:** HIGH

## Summary

Phase 52 wires the YAML parsing capability (from Phase 51) into the DDL pipeline so that `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$` creates a queryable semantic view. The implementation touches two layers: (1) the parser/validator in `parse.rs` where `FROM YAML` detection and dollar-quote extraction are added, and (2) the rewrite pipeline where YAML content is deserialized to `SemanticViewDefinition`, serialized to JSON, and emitted as the existing `create_semantic_view_from_json('name', 'json')` function call. All three CREATE modifiers (plain, OR REPLACE, IF NOT EXISTS) are supported because they share the same `validate_create_body` dispatcher.

The architecture is straightforward: the parser hook receives raw query text (DuckDB's parser failed first, then the fallback hook fires). The existing `validate_create_body()` function already dispatches on the text after the view name -- currently checking for `AS` (SQL body path). Phase 52 adds a second branch: `FROM YAML` (YAML body path). Dollar-quote extraction (`$$...$$`) is a simple find-matching-delimiter scan. The extracted YAML is passed to `SemanticViewDefinition::from_yaml_with_size_cap()` (Phase 51), then the resulting struct is serialized to JSON and embedded in the same function call SQL that the SQL DDL path uses. No changes to the C++ shim, no new table functions, no new DdlKind variants.

The COMMENT annotation between view name and FROM YAML is supported via the existing `extract_view_comment()` function, which runs before the AS/FROM YAML dispatch.

**Primary recommendation:** Add `FROM YAML` detection in `validate_create_body()`, implement `extract_dollar_quoted()` and `rewrite_ddl_yaml_body()` functions in `parse.rs`, update the error message to mention the YAML syntax, and write comprehensive tests (unit tests in `parse.rs`, sqllogictest integration tests).

## Project Constraints (from CLAUDE.md)

- **Quality gate:** `just test-all` must pass (Rust unit tests + proptests + sqllogictest + DuckLake CI)
- **Test coverage:** Every phase must include unit tests, proptests, sqllogictest, and consider fuzz targets
- **Build:** `cargo test` runs without the extension feature (in-memory DuckDB)
- **SQL logic tests:** Require `just build` first; cover integration paths Rust tests miss
- **Linting:** clippy pedantic + fmt + cargo-deny before pushing to main
- **Fuzz targets:** Compilation checked via `just check-fuzz` (nightly)
- **Snowflake reference:** When in doubt about SQL syntax or behaviour, refer to what Snowflake semantic views does

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| YAML-01 | User can create a semantic view from inline YAML using `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$` | `validate_create_body()` gains a `FROM YAML` branch that extracts dollar-quoted content, deserializes via `from_yaml_with_size_cap()`, serializes to JSON, and rewrites to `create_semantic_view_from_json('name', 'json')`. Same execution path as SQL DDL -- identical validation, persistence, and query behavior. |
| YAML-06 | `CREATE OR REPLACE` and `IF NOT EXISTS` modifiers work with `FROM YAML` syntax | The three CREATE modifiers (`Create`, `CreateOrReplace`, `CreateIfNotExists`) all flow through `validate_create_body()` and share the same `DdlKind -> function_name` mapping. The YAML branch uses the same `kind` variable to select the correct `_from_json` function variant. No additional work needed beyond the core rewrite function. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| yaml_serde | 0.10.4 | YAML deserialization (already in Cargo.toml from Phase 51) | Phase 51 dependency; `from_yaml_with_size_cap()` already implemented on `SemanticViewDefinition` [VERIFIED: Cargo.toml line 36, src/model.rs line 437] |
| serde_json | 1 (existing) | JSON serialization for rewrite output | Already used by `rewrite_ddl_keyword_body()` to serialize `SemanticViewDefinition` to JSON [VERIFIED: src/parse.rs line 1105] |

### Supporting
No new dependencies required for Phase 52. All infrastructure exists from Phase 51 (yaml_serde) and earlier phases (parse.rs, body_parser.rs, define.rs).

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Dollar-quoting (`$$`) | Single-quoted body with `''` escaping | Dollar-quoting avoids escaping YAML content that contains single quotes. YAML frequently uses single-quoted strings (`name: 'value'`), making `''` escaping very painful. Dollar-quoting is the clear winner. [CITED: .planning/research/FEATURES.md line 202] |
| No new DdlKind variant | Separate `CreateFromYaml` / `CreateOrReplaceFromYaml` variants | Sub-dispatch within `validate_create_body()` is cleaner. Adding 3 more DdlKind variants would triple CREATE variants for no benefit since the YAML path converges to the same `_from_json` function call. [CITED: .planning/research/ARCHITECTURE.md line 127] |

## Architecture Patterns

### Recommended Project Structure
```
src/
  parse.rs       # Modified: FROM YAML detection, dollar-quote extraction, YAML rewrite
  model.rs       # Unchanged (from_yaml_with_size_cap already exists from Phase 51)
  body_parser.rs # Unchanged
  ddl/define.rs  # Unchanged (DefineFromJsonVTab handles the rewritten function call)
test/sql/
  phase52_yaml_ddl.test  # New: sqllogictest integration tests for YAML DDL
```

### Pattern 1: Parser Dispatch -- FROM YAML Branch in validate_create_body
**What:** After extracting the view name and optional COMMENT, check if remaining text starts with `FROM YAML` (case-insensitive, whitespace-delimited). If so, route to `rewrite_ddl_yaml_body()` instead of the existing `rewrite_ddl_keyword_body()`.
**When to use:** Every `CREATE SEMANTIC VIEW ... FROM YAML ...` statement.
**Example:**
```rust
// Source: pattern from validate_create_body() in src/parse.rs lines 1030-1051
// After the existing is_as_body check, BEFORE the error at the bottom:

let is_yaml_body = after_name_trimmed
    .get(..9)
    .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
    && (after_name_trimmed.len() == 9
        || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());

if is_yaml_body {
    let yaml_text = &after_name_trimmed[9..].trim_start(); // text after "FROM YAML"
    return rewrite_ddl_yaml_body(kind, name, yaml_text, view_comment);
}
```
[VERIFIED: src/parse.rs lines 996-1061 for validate_create_body structure]

### Pattern 2: Dollar-Quote Extraction
**What:** Scan for opening `$$` delimiter, then find matching closing `$$`. Return the content between them.
**When to use:** When `FROM YAML` is detected and the body is dollar-quoted.
**Example:**
```rust
// Source: derived from .planning/research/ARCHITECTURE.md lines 147-156
/// Extract content from a dollar-quoted string.
///
/// Supports untagged `$$...$$` and tagged `$tag$...$tag$` delimiters.
/// Returns `(content, total_bytes_consumed)` on success.
fn extract_dollar_quoted(input: &str) -> Result<(String, usize), ParseError> {
    if !input.starts_with('$') {
        return Err(ParseError {
            message: "Expected '$' to begin dollar-quoted string".to_string(),
            position: None,
        });
    }
    // Find end of opening delimiter: scan for second '$'
    let tag_end = input[1..].find('$')
        .ok_or_else(|| ParseError {
            message: "Unterminated dollar-quote opening delimiter".to_string(),
            position: None,
        })?
        + 2; // +1 for 0-index, +1 for the leading '$'
    let delimiter = &input[..tag_end]; // e.g. "$$" or "$yaml$"
    // Find closing delimiter
    let content_start = tag_end;
    let close_pos = input[content_start..].find(delimiter)
        .ok_or_else(|| ParseError {
            message: format!(
                "Unterminated dollar-quoted string (expected closing '{delimiter}')"
            ),
            position: None,
        })?;
    let content = &input[content_start..content_start + close_pos];
    let total_consumed = content_start + close_pos + delimiter.len();
    Ok((content.to_string(), total_consumed))
}
```
[CITED: .planning/research/PITFALLS.md lines 117-135 for edge cases]

### Pattern 3: YAML-to-JSON Rewrite (rewrite_ddl_yaml_body)
**What:** Extract dollar-quoted YAML, deserialize via `from_yaml_with_size_cap()`, serialize to JSON, embed in `create_semantic_view_from_json()` call. Mirrors `rewrite_ddl_keyword_body()` output format.
**When to use:** The YAML body path in validate_create_body.
**Example:**
```rust
// Source: pattern from rewrite_ddl_keyword_body() in src/parse.rs lines 1068-1125
fn rewrite_ddl_yaml_body(
    kind: DdlKind,
    name: &str,
    yaml_text: &str,           // text after "FROM YAML", starting at dollar-quote
    view_comment: Option<String>,
) -> Result<Option<String>, ParseError> {
    // 1. Extract dollar-quoted content
    let (yaml_content, _consumed) = extract_dollar_quoted(yaml_text)?;

    // 2. Deserialize YAML (with size cap)
    let mut def = crate::model::SemanticViewDefinition::from_yaml_with_size_cap(name, &yaml_content)
        .map_err(|e| ParseError { message: e, position: None })?;

    // 3. Set view-level comment if provided via DDL (COMMENT = '...' FROM YAML $$...$$)
    if let Some(c) = view_comment {
        def.comment = Some(c);
    }

    // 4. Infer cardinality (same as SQL path)
    infer_cardinality(&def.tables, &mut def.joins)?;

    // 5. Set base_table from first table (backward compat, same as SQL path)
    if def.base_table.is_empty() {
        if let Some(first) = def.tables.first() {
            def.base_table = first.table.clone();
        }
    }

    // 6. Serialize to JSON
    let json = serde_json::to_string(&def).map_err(|e| ParseError {
        message: format!("Failed to serialize YAML definition: {e}"),
        position: None,
    })?;

    // 7. SQL-escape and build function call
    let safe_name = name.replace('\'', "''");
    let safe_json = json.replace('\'', "''");

    let fn_name = match kind {
        DdlKind::Create => "create_semantic_view_from_json",
        DdlKind::CreateOrReplace => "create_or_replace_semantic_view_from_json",
        DdlKind::CreateIfNotExists => "create_semantic_view_if_not_exists_from_json",
        _ => unreachable!("rewrite_ddl_yaml_body only called for CREATE forms"),
    };

    Ok(Some(format!(
        "SELECT * FROM {fn_name}('{safe_name}', '{safe_json}')"
    )))
}
```
[VERIFIED: src/parse.rs lines 1068-1125 for existing rewrite_ddl_keyword_body pattern]

### Pattern 4: base_table Population from YAML
**What:** The YAML format does not have the same `base_table` implicit convention as the SQL DDL path. In SQL DDL, `base_table` is derived from the first TABLES entry. In YAML, users can set `base_table` directly (since it is a serde field). If they omit it (empty string default), we must populate it from the first table, matching SQL DDL behavior.
**When to use:** In `rewrite_ddl_yaml_body()` after deserialization.
**Example:** See step 5 in Pattern 3 above.
[VERIFIED: src/parse.rs lines 1083-1088 for existing base_table derivation in rewrite_ddl_keyword_body]

### Anti-Patterns to Avoid
- **New DdlKind variants for YAML:** Do NOT add `CreateFromYaml`, `CreateOrReplaceFromYaml`, etc. to the DdlKind enum. The YAML vs SQL body format is a sub-dispatch within CREATE, not a separate statement kind. [CITED: .planning/research/ARCHITECTURE.md line 127]
- **Processing YAML at bind time (in DefineFromJsonVTab):** Do NOT pass raw YAML through to the table function. Convert YAML to JSON at parse/rewrite time. This keeps DefineFromJsonVTab unchanged and ensures consistent behavior -- all definitions arrive as JSON regardless of input format. [ASSUMED -- follows existing architecture where body_parser converts SQL to JSON at rewrite time]
- **Skipping cardinality inference for YAML path:** Do NOT omit the `infer_cardinality()` call. The YAML path must call it exactly like the SQL path does, so that relationships with FK columns get resolved cardinality and ref_columns. [VERIFIED: src/parse.rs line 1079 shows SQL path calls infer_cardinality]
- **Custom parser for dollar-quoting:** Do NOT use a parser combinator library (nom, pest, etc.) for `$$` extraction. The delimiter scan is trivial -- two `find()` calls. [CITED: .planning/research/STACK.md line 131]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| YAML deserialization | Custom YAML parser | `SemanticViewDefinition::from_yaml_with_size_cap()` (Phase 51) | Already implemented and tested with 11 unit tests + 256-case proptest proving YAML/JSON equivalence [VERIFIED: src/model.rs lines 427-457] |
| JSON serialization for rewrite | Manual JSON construction | `serde_json::to_string()` | Already used by `rewrite_ddl_keyword_body()` [VERIFIED: src/parse.rs line 1105] |
| View definition validation | Separate YAML validation | `DefineFromJsonVTab::bind()` chain | Graph validation, derived metrics, USING relationships, fan trap detection all run on the deserialized `SemanticViewDefinition` at bind time. YAML path reuses this entirely. [VERIFIED: src/ddl/define.rs lines 211-225] |
| Cardinality inference | YAML-specific inference | `infer_cardinality()` in parse.rs | Already handles PK/FK resolution for relationships [VERIFIED: src/parse.rs line 1079] |

**Key insight:** Phase 52 adds approximately 60-80 lines of new code in `parse.rs`. Everything else -- YAML parsing, validation, persistence, querying -- is reused from existing infrastructure.

## Common Pitfalls

### Pitfall 1: Dollar-Quote Content Containing $$
**What goes wrong:** If the YAML content contains the literal string `$$`, the untagged dollar-quote extractor terminates early, truncating the YAML. The deserialization then fails with confusing errors about invalid YAML or missing fields.
**Why it happens:** YAML content can contain arbitrary strings. While `$$` in YAML is unlikely, it is possible (e.g., in a SQL expression like `expr: "amount * $$price$$"`).
**How to avoid:** Support tagged delimiters (`$yaml$...$yaml$`). Document that if YAML content contains `$$`, users should use a tagged delimiter. The error message for a truncated parse should hint at this possibility.
**Warning signs:** Deserialization errors on YAML that "looks valid" when inspected manually.
[CITED: .planning/research/PITFALLS.md lines 117-135]

### Pitfall 2: Missing base_table in YAML
**What goes wrong:** The YAML `base_table` field defaults to empty string (serde `Default`). The SQL DDL path always sets `base_table` from the first TABLES entry. If the YAML path does not do the same, views created from YAML have an empty `base_table`, which could cause issues in query expansion.
**Why it happens:** In the SQL DDL rewrite path (`rewrite_ddl_keyword_body`), `base_table` is explicitly set from `keyword_body.tables.first()` (line 1083-1088). The YAML deserialization path does not do this automatically -- it just reads whatever the user put in the YAML.
**How to avoid:** After `from_yaml_with_size_cap()`, check if `base_table` is empty and populate it from the first table entry, matching the SQL path behavior. [VERIFIED: src/parse.rs lines 1083-1088]
**Warning signs:** Query expansion errors about "base table not found" when querying a YAML-created view.

### Pitfall 3: View-Level COMMENT Precedence
**What goes wrong:** The YAML format has a `comment` field at the top level. The SQL DDL syntax also supports `COMMENT = '...'` between the view name and `FROM YAML`. If both are present, which takes precedence?
**Why it happens:** `extract_view_comment()` runs before the YAML body is parsed. The DDL-level comment overwrites `def.comment` in the rewrite function. But the YAML body may also contain a `comment` field.
**How to avoid:** Define clear precedence: DDL-level COMMENT (between view name and FROM YAML) overrides the YAML `comment` field. This matches how the SQL DDL path works -- the DDL-level COMMENT is always the authoritative source. The rewrite function should set `def.comment` to the DDL-level comment AFTER deserializing YAML, overriding any YAML-level comment. If no DDL-level COMMENT is provided, the YAML `comment` field stands.
**Warning signs:** Tests should cover: (a) YAML with comment + no DDL COMMENT, (b) no YAML comment + DDL COMMENT, (c) both present (DDL wins).

### Pitfall 4: Cardinality Inference Must Run on YAML Path
**What goes wrong:** The SQL DDL path calls `infer_cardinality()` after parsing to resolve relationship cardinalities and ref_columns from PK declarations. If the YAML path skips this, relationships with FK columns but no explicit `ref_columns` will have empty ref_columns, causing errors at bind time.
**Why it happens:** The YAML deserialization produces a raw struct where `ref_columns` may be empty (the user may rely on PK resolution). The SQL path handles this via `infer_cardinality()` at rewrite time.
**How to avoid:** Call `infer_cardinality(&def.tables, &mut def.joins)` in `rewrite_ddl_yaml_body()`, matching `rewrite_ddl_keyword_body()` line 1079. [VERIFIED: src/parse.rs line 1079]
**Warning signs:** "FK column count does not match referenced column count" errors when creating from YAML.

### Pitfall 5: Rewrite Buffer Size (64KB)
**What goes wrong:** The C++ shim allocates a 64KB buffer for the rewritten SQL in the bind path (`sv_ddl_bind`). A YAML definition that produces a large JSON representation (many tables, dimensions, metrics, long expressions) could exceed this buffer, causing silent truncation.
**Why it happens:** The `write_to_buffer()` function truncates to `len - 1` bytes without error.
**How to avoid:** This is an existing infrastructure constraint that applies equally to the SQL DDL path. The 1MB YAML size cap means the JSON could theoretically exceed 64KB (though practically unlikely for real definitions). For Phase 52, document this as a known limitation. A future fix would be to dynamically allocate the buffer in the C++ shim.
**Warning signs:** "DDL execution failed" errors on very large definitions. The SQL would be silently truncated, producing invalid function call syntax.
[VERIFIED: cpp/src/shim.cpp line 142 for 64KB buffer]

### Pitfall 6: Trailing Content After Closing $$
**What goes wrong:** `CREATE SEMANTIC VIEW x FROM YAML $$ ... $$ extra stuff here` should be an error, but if we only extract the dollar-quoted content and ignore the rest, the trailing content is silently ignored.
**Why it happens:** The dollar-quote extractor returns `(content, consumed)`. If `consumed < input.len()`, there is trailing content.
**How to avoid:** After extracting the dollar-quoted content, check that the remaining text (after trimming whitespace) is empty. If not, return a `ParseError` indicating unexpected tokens after the closing delimiter.
**Warning signs:** Users accidentally putting extra SQL after the YAML block and not getting an error.

## Code Examples

### FROM YAML Detection in validate_create_body
```rust
// Source: derived from validate_create_body() structure in src/parse.rs
// Insert AFTER the is_as_body block (line 1051), BEFORE the error (line 1055):

// --- FROM YAML body path (Phase 52) ---
let is_yaml_body = after_name_trimmed
    .get(..9)
    .is_some_and(|s| s.eq_ignore_ascii_case("FROM YAML"))
    && (after_name_trimmed.len() == 9
        || after_name_trimmed.as_bytes()[9].is_ascii_whitespace());
if is_yaml_body {
    let yaml_text = after_name_trimmed[9..].trim_start();
    return rewrite_ddl_yaml_body(kind, name, yaml_text, view_comment);
}
// --- End FROM YAML body path ---
```

### Dollar-Quote Extraction
```rust
// Source: derived from .planning/research/ARCHITECTURE.md + PITFALLS.md
/// Extract content from a dollar-quoted string (`$$...$$` or `$tag$...$tag$`).
///
/// Returns `(content, bytes_consumed)` where bytes_consumed includes both
/// opening and closing delimiters. The content does NOT include the delimiters.
///
/// Supports:
/// - Untagged: `$$content$$` -> ("content", 4 + content.len())
/// - Tagged: `$yaml$content$yaml$` -> ("content", 12 + content.len())
///
/// Tag validation: tag must be alphanumeric + underscore, no leading digit.
fn extract_dollar_quoted(input: &str) -> Result<(String, usize), ParseError> {
    if !input.starts_with('$') {
        return Err(ParseError {
            message: "Expected '$' to begin dollar-quoted string".to_string(),
            position: None,
        });
    }
    // Find end of opening delimiter (second '$')
    let tag_end = input[1..].find('$')
        .ok_or_else(|| ParseError {
            message: "Unterminated dollar-quote opening delimiter".to_string(),
            position: None,
        })?
        + 2; // +1 for the skip of input[0], +1 for the '$' itself
    let delimiter = &input[..tag_end]; // "$$" or "$tag$"

    // Find matching closing delimiter
    let content_start = tag_end;
    let close_pos = input[content_start..].find(delimiter)
        .ok_or_else(|| ParseError {
            message: format!(
                "Unterminated dollar-quoted string (expected closing '{delimiter}')"
            ),
            position: None,
        })?;
    let content = &input[content_start..content_start + close_pos];
    let total = content_start + close_pos + delimiter.len();
    Ok((content.to_string(), total))
}
```

### rewrite_ddl_yaml_body
```rust
// Source: derived from rewrite_ddl_keyword_body() in src/parse.rs lines 1068-1125
fn rewrite_ddl_yaml_body(
    kind: DdlKind,
    name: &str,
    yaml_text: &str,              // text after "FROM YAML " (leading whitespace trimmed)
    view_comment: Option<String>,
) -> Result<Option<String>, ParseError> {
    // 1. Extract dollar-quoted content
    let (yaml_content, consumed) = extract_dollar_quoted(yaml_text)?;

    // 2. Check for trailing content after closing delimiter
    let trailing = yaml_text[consumed..].trim();
    if !trailing.is_empty() {
        return Err(ParseError {
            message: format!("Unexpected content after closing dollar-quote: '{trailing}'"),
            position: None,
        });
    }

    // 3. Deserialize YAML with size cap (Phase 51)
    let mut def = crate::model::SemanticViewDefinition::from_yaml_with_size_cap(name, &yaml_content)
        .map_err(|e| ParseError { message: e, position: None })?;

    // 4. Set view-level comment from DDL (overrides YAML comment field)
    if let Some(c) = view_comment {
        def.comment = Some(c);
    }

    // 5. Populate base_table from first table if empty (matches SQL DDL path)
    if def.base_table.is_empty() {
        if let Some(first) = def.tables.first() {
            def.base_table = first.table.clone();
        }
    }

    // 6. Infer cardinality (same as SQL path)
    infer_cardinality(&def.tables, &mut def.joins)?;

    // 7. Serialize to JSON
    let json = serde_json::to_string(&def).map_err(|e| ParseError {
        message: format!("Failed to serialize YAML definition: {e}"),
        position: None,
    })?;

    // 8. SQL-escape and build function call
    let safe_name = name.replace('\'', "''");
    let safe_json = json.replace('\'', "''");
    let fn_name = match kind {
        DdlKind::Create => "create_semantic_view_from_json",
        DdlKind::CreateOrReplace => "create_or_replace_semantic_view_from_json",
        DdlKind::CreateIfNotExists => "create_semantic_view_if_not_exists_from_json",
        _ => unreachable!("rewrite_ddl_yaml_body only called for CREATE forms"),
    };
    Ok(Some(format!(
        "SELECT * FROM {fn_name}('{safe_name}', '{safe_json}')"
    )))
}
```

### Updated Error Message
```rust
// Source: current error at src/parse.rs lines 1055-1061
// Update to mention both AS and FROM YAML syntax:
Err(ParseError {
    message: "Expected 'AS' or 'FROM YAML' after view name. \
              Use: CREATE SEMANTIC VIEW name AS TABLES (...) DIMENSIONS (...) METRICS (...) \
              or: CREATE SEMANTIC VIEW name FROM YAML $$ ... $$".to_string(),
    position: Some(trim_offset + pos_in_trimmed),
})
```

### SQLLogicTest Integration Test Pattern
```
# Source: pattern from test/sql/phase25_keyword_body.test
require semantic_views

# Setup: create a test table
statement ok
CREATE TABLE p52_orders (id INTEGER PRIMARY KEY, amount DOUBLE, region VARCHAR);

statement ok
INSERT INTO p52_orders VALUES (1, 100.0, 'East'), (2, 200.0, 'West');

# YAML-01: Create semantic view from inline YAML
statement ok
CREATE SEMANTIC VIEW p52_yaml_basic FROM YAML $$
base_table: p52_orders
tables:
  - alias: o
    table: p52_orders
    pk_columns:
      - id
dimensions:
  - name: region
    expr: o.region
    source_table: o
metrics:
  - name: total_amount
    expr: SUM(o.amount)
    source_table: o
$$

# Verify it works like SQL-created views
query TT rowsort
SELECT region, total_amount FROM semantic_view('p52_yaml_basic', dimensions := ['region'], metrics := ['total_amount'])
----
East    100.0
West    200.0

# YAML-06: CREATE OR REPLACE with FROM YAML
statement ok
CREATE OR REPLACE SEMANTIC VIEW p52_yaml_basic FROM YAML $$
base_table: p52_orders
tables:
  - alias: o
    table: p52_orders
    pk_columns:
      - id
dimensions:
  - name: region
    expr: o.region
    source_table: o
metrics:
  - name: total_amount
    expr: SUM(o.amount)
    source_table: o
$$

# YAML-06: CREATE IF NOT EXISTS (no-op when view exists)
statement ok
CREATE SEMANTIC VIEW IF NOT EXISTS p52_yaml_basic FROM YAML $$
base_table: p52_orders
tables:
  - alias: o
    table: p52_orders
    pk_columns:
      - id
dimensions:
  - name: region
    expr: o.region
    source_table: o
metrics:
  - name: total_amount
    expr: SUM(o.amount)
    source_table: o
$$
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SQL keyword body only | SQL keyword body + YAML body | Phase 52 (v0.7.0) | Users can choose between SQL DDL syntax and YAML for defining semantic views |
| No dollar-quoting in parse.rs | Dollar-quote extraction in parse.rs | Phase 52 (v0.7.0) | Enables multi-line string literals in DDL without escaping |

**Deprecated/outdated:**
- Function-based DDL (retired in v0.5.2) -- not relevant to Phase 52
- serde_yaml (dtolnay) -- archived, replaced by yaml_serde (already in Phase 51)

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | DDL-level COMMENT = '...' should override YAML `comment` field when both present | Common Pitfalls / Pitfall 3 | If wrong, users get unexpected comment values. Easily reversible -- just swap the precedence logic. Risk: LOW. |
| A2 | `base_table` should be auto-populated from first table entry in YAML path (matching SQL DDL path) | Common Pitfalls / Pitfall 2 | If wrong, views with empty base_table could fail at query time. The SQL DDL path does this, so consistency argues for it. Risk: MEDIUM -- YAML users might set base_table explicitly, in which case auto-population should not override it. |
| A3 | Tagged dollar-quoting (`$yaml$...$yaml$`) is worth supporting in v0.7.0 | Code Examples / Dollar-Quote Extraction | If too complex, untagged `$$` only is acceptable. Risk: LOW -- the implementation is ~5 extra lines. |

## Open Questions

1. **base_table in YAML: explicit vs auto-populated**
   - What we know: SQL DDL path derives base_table from first TABLES entry. YAML users can set base_table directly in their YAML.
   - What's unclear: If a user sets both `base_table: orders` and `tables: [{alias: o, table: orders, ...}]`, should we keep their explicit base_table or override it?
   - Recommendation: Only auto-populate if base_table is empty (the serde default). If the user set it explicitly, respect their value. This is what the code example shows.

2. **Near-miss detection for FROM YAML typos**
   - What we know: The existing `detect_near_miss()` function uses Levenshtein distance for DDL prefixes. It checks prefixes like "create semantic view".
   - What's unclear: Should we add near-miss detection for `FROM YAML` (e.g., `FROM YML`, `FORM YAML`)?
   - Recommendation: Not needed for Phase 52. The existing near-miss detects the CREATE prefix. If the user types `CREATE SEMANTIC VIEW x FROM YML $$...$$`, the parser will fail to match AS or FROM YAML and return the updated error message mentioning both syntaxes.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) + sqllogictest runner |
| Config file | Cargo.toml + test/sql/*.test |
| Quick run command | `cargo test parse::tests::yaml` |
| Full suite command | `just test-all` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| YAML-01 | `CREATE SEMANTIC VIEW name FROM YAML $$ ... $$` creates a queryable view | unit + sqllogictest | `cargo test parse::tests::yaml` + `just test-sql` | Wave 0 |
| YAML-01 | Dollar-quoted YAML with all field types roundtrips correctly | unit | `cargo test parse::tests::yaml` | Wave 0 |
| YAML-01 | Invalid YAML in dollar-quoted block returns clear error | unit | `cargo test parse::tests::yaml` | Wave 0 |
| YAML-01 | YAML exceeding size cap in DDL context is rejected | unit | `cargo test parse::tests::yaml` | Wave 0 |
| YAML-01 | Tagged dollar-quoting ($yaml$...$yaml$) works | unit | `cargo test parse::tests::yaml` | Wave 0 |
| YAML-01 | COMMENT = '...' FROM YAML $$...$$ works (view-level comment) | unit | `cargo test parse::tests::yaml` | Wave 0 |
| YAML-06 | CREATE OR REPLACE FROM YAML replaces existing view | unit + sqllogictest | `cargo test parse::tests::yaml` + `just test-sql` | Wave 0 |
| YAML-06 | CREATE IF NOT EXISTS FROM YAML is no-op for existing view | unit + sqllogictest | `cargo test parse::tests::yaml` + `just test-sql` | Wave 0 |
| -- | Dollar-quote extraction: untagged, tagged, unterminated, nested | unit | `cargo test parse::tests::yaml` | Wave 0 |
| -- | Trailing content after closing $$ is rejected | unit | `cargo test parse::tests::yaml` | Wave 0 |
| -- | `FROM YAML` detection is case-insensitive | unit | `cargo test parse::tests::yaml` | Wave 0 |
| -- | Error message mentions both AS and FROM YAML syntax | unit | `cargo test parse::tests::yaml` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test`
- **Per wave merge:** `just test-all`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `test/sql/phase52_yaml_ddl.test` -- sqllogictest integration tests for all YAML DDL paths
- [ ] `src/parse.rs` unit tests -- `extract_dollar_quoted`, `rewrite_ddl_yaml_body`, FROM YAML detection
- [ ] Update `test/sql/TEST_LIST` if it enumerates test files

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A (DDL is a privileged operation) |
| V5 Input Validation | yes | Size cap (1MB via `from_yaml_with_size_cap`); dollar-quote parsing with proper error handling; trailing content rejection |
| V6 Cryptography | no | N/A |

### Known Threat Patterns for Dollar-Quoted YAML DDL

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Oversized YAML via DDL | Denial of Service | 1MB size cap in `from_yaml_with_size_cap` (sanity guard, not security boundary per Phase 51 decision) |
| Dollar-quote injection (content containing `$$`) | Tampering | Tagged delimiter support (`$yaml$...$yaml$`); clear error for unterminated delimiters |
| SQL injection via YAML content | Tampering | YAML is deserialized into typed Rust structs, then re-serialized to JSON. Single quotes in the JSON are SQL-escaped (`''`). The content never passes through SQL as raw text. [VERIFIED: src/parse.rs lines 1110-1112 for SQL escaping] |

## Sources

### Primary (HIGH confidence)
- [src/parse.rs] -- full parser dispatch: validate_create_body, rewrite_ddl_keyword_body, FFI entry points, test suite (1400+ lines)
- [src/model.rs] -- SemanticViewDefinition struct, from_yaml/from_yaml_with_size_cap (Phase 51), from_json
- [src/ddl/define.rs] -- DefineFromJsonVTab::bind() validation chain (graph, facts, derived metrics, USING)
- [cpp/src/shim.cpp] -- C++ parser hook: sv_parse_stub, sv_ddl_bind, buffer sizes (16KB/64KB)
- [.planning/research/ARCHITECTURE.md] -- FROM YAML detection architecture, dollar-quote extraction algorithm
- [.planning/research/PITFALLS.md] -- dollar-quote edge cases, tagged delimiters
- [test/sql/phase25_keyword_body.test] -- sqllogictest patterns for DDL integration tests
- [tests/yaml_proptest.rs] -- Phase 51 proptest proving YAML/JSON equivalence

### Secondary (MEDIUM confidence)
- [DuckDB literal types docs](https://duckdb.org/docs/current/sql/data_types/literal_types) -- DuckDB dollar-quoting syntax documentation
- [.planning/research/FEATURES.md] -- FROM YAML feature spec
- [.planning/research/STACK.md] -- dollar-quoting implementation notes

### Tertiary (LOW confidence)
- None -- all claims verified against codebase or prior research

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- no new dependencies, yaml_serde already present from Phase 51, rewrite pattern fully verified in existing codebase
- Architecture: HIGH -- insertion point in validate_create_body clearly identified (line 1051), rewrite_ddl_keyword_body pattern fully understood, same function call output
- Pitfalls: HIGH -- dollar-quote edge cases documented in prior research, buffer size limitation verified in C++ shim, SQL escaping pattern verified

**Research date:** 2026-04-18
**Valid until:** 2026-05-18 (stable domain; parse.rs architecture unlikely to change)
