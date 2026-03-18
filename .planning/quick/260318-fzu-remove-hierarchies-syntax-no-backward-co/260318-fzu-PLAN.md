---
phase: quick
plan: 260318-fzu
type: execute
wave: 1
depends_on: []
files_modified:
  - src/model.rs
  - src/body_parser.rs
  - src/graph.rs
  - src/ddl/describe.rs
  - src/ddl/define.rs
  - src/parse.rs
  - src/expand.rs
  - tests/parse_proptest.rs
  - tests/expand_proptest.rs
  - test/sql/phase29_facts_hierarchies.test
  - test/sql/phase20_extended_ddl.test
  - test/sql/phase21_error_reporting.test
  - test/sql/phase25_keyword_body.test
  - test/sql/phase28_e2e.test
  - test/sql/phase30_derived_metrics.test
  - examples/advanced_features.py
  - README.md
  - fuzz/seeds/fuzz_ddl_parse/seed_hierarchies.txt
  - fuzz/seeds/fuzz_ddl_parse/seed_facts_and_hierarchies.txt
autonomous: true
requirements: []

must_haves:
  truths:
    - "HIERARCHIES keyword is rejected by the parser with a clear error"
    - "DESCRIBE SEMANTIC VIEW returns 7 columns (hierarchies column removed)"
    - "All existing non-hierarchy functionality works unchanged"
    - "just test-all passes"
  artifacts:
    - path: "src/model.rs"
      provides: "SemanticViewDefinition without Hierarchy struct or hierarchies field"
    - path: "src/body_parser.rs"
      provides: "Parser without HIERARCHIES clause support"
    - path: "src/ddl/describe.rs"
      provides: "7-column DESCRIBE output (no hierarchies column)"
  key_links:
    - from: "src/body_parser.rs"
      to: "src/model.rs"
      via: "KeywordBody struct no longer has hierarchies field"
      pattern: "pub struct KeywordBody"
    - from: "src/ddl/describe.rs"
      to: "DESCRIBE output"
      via: "7 columns instead of 8"
      pattern: "flat_vector"
---

<objective>
Remove the HIERARCHIES clause entirely from CREATE SEMANTIC VIEW syntax.

Purpose: HIERARCHIES was pure metadata that added complexity without query-time value. Removing it simplifies the DDL surface before registry publishing.
Output: Clean codebase with no hierarchy support, all tests passing.
</objective>

<execution_context>
@/Users/paul/.claude/get-shit-done/workflows/execute-plan.md
@/Users/paul/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src/model.rs
@src/body_parser.rs
@src/graph.rs
@src/ddl/describe.rs
@src/ddl/define.rs
@src/parse.rs
@src/expand.rs
@tests/parse_proptest.rs
@tests/expand_proptest.rs
@test/sql/phase29_facts_hierarchies.test
@test/sql/phase20_extended_ddl.test
@test/sql/phase21_error_reporting.test
@test/sql/phase25_keyword_body.test
@test/sql/phase28_e2e.test
@test/sql/phase30_derived_metrics.test
@examples/advanced_features.py
@README.md
</context>

<tasks>

<task type="auto">
  <name>Task 1: Remove Hierarchy from Rust source code</name>
  <files>src/model.rs, src/body_parser.rs, src/graph.rs, src/ddl/describe.rs, src/ddl/define.rs, src/parse.rs, src/expand.rs, tests/parse_proptest.rs, tests/expand_proptest.rs</files>
  <action>
Remove all hierarchy support from the Rust codebase. This is a systematic deletion across multiple files:

**src/model.rs:**
- Delete the `Hierarchy` struct (around line 85) and its doc comments (lines 79-84)
- Remove `hierarchies: Vec<Hierarchy>` field from `SemanticViewDefinition` (line 194) and its serde annotations
- Remove `hierarchies: vec![]` from all test helper constructions
- Delete the entire `phase29_hierarchy_tests` module (starts around line 452)
- Remove `Hierarchy` from any `use` imports

**src/body_parser.rs:**
- Remove `Hierarchy` from the `use crate::model` import (line 6)
- Remove `pub hierarchies: Vec<Hierarchy>` from `KeywordBody` struct (line 15)
- Remove `"hierarchies"` from `CLAUSE_KEYWORDS` array (line 25)
- Remove `"hierarchies"` from `CLAUSE_ORDER` array (line 37)
- Update the error message strings that list clause keywords — change from "TABLES, RELATIONSHIPS, FACTS, HIERARCHIES, DIMENSIONS, METRICS" to "TABLES, RELATIONSHIPS, FACTS, DIMENSIONS, METRICS" (lines ~140, ~164, ~263)
- Remove `let mut hierarchies: Vec<Hierarchy> = Vec::new();` (line 318) and the `"hierarchies" =>` match arm (lines 333-334)
- Remove `hierarchies,` from the KeywordBody return (line 381)
- Delete `parse_hierarchies_clause()` function (starts around line 1061)
- Delete `parse_single_hierarchy_entry()` function (starts around line 1082)
- Delete ALL hierarchy-related unit tests: `parse_keyword_body_with_hierarchies_single`, `parse_keyword_body_with_hierarchies_single_level`, `parse_keyword_body_with_empty_hierarchies`, `parse_keyword_body_hierarchy_without_parens_rejected`, `parse_keyword_body_hierarchy_with_empty_parens_rejected`, `parse_keyword_body_with_facts_and_hierarchies`, `parse_keyword_body_hierarchies_after_dimensions_rejected`, `parse_hierarchies_clause_empty_body`, `parse_hierarchies_clause_single`, `parse_hierarchies_clause_multiple`, `parse_hierarchies_clause_lowercase_as`
- Update `parse_keyword_body_with_facts_and_hierarchies` test -- the ordering test that checks "FACTS must come before DIMENSIONS" is still valid, just remove the HIERARCHIES from it and rename it. Actually, the test at line 1851 already tests FACTS ordering without hierarchies, so just delete the one at line 1840.

**src/graph.rs:**
- Delete the entire `validate_hierarchies()` function (starts around line 592) and its doc comments
- Delete ALL hierarchy-related test functions: `validate_hierarchies_empty_returns_ok`, `validate_hierarchies_valid_hierarchy`, `validate_hierarchies_unknown_dimension`, `validate_hierarchies_unknown_dimension_fuzzy_suggestion`, `validate_hierarchies_case_insensitive`
- Remove `hierarchies: vec![]` from all test helper constructions in this file
- Remove `Hierarchy` from any imports if present

**src/ddl/define.rs:**
- Remove the call to `crate::graph::validate_hierarchies(&def)` (around line 132-133)

**src/parse.rs:**
- Remove `hierarchies: keyword_body.hierarchies` from the SemanticViewDefinition construction (line 480)

**src/ddl/describe.rs:**
- Remove the `hierarchies: String` field from `DescribeBindData` struct (line 23)
- Remove the `bind.add_result_column("hierarchies", ...)` call (lines 71-74)
- Remove the `hierarchies` variable computation (lines 103-107)
- Remove `hierarchies,` from the `Ok(DescribeBindData { ... })` return (line 117)
- Remove `let hierarchies_vec = output.flat_vector(7);` (line 146)
- Remove `hierarchies_vec.insert(0, bind_data.hierarchies.as_str());` (line 155)
- Update doc comments: change "8 columns" to "7 columns", remove "hierarchies" from the schema description (lines 12-14, 42-47)

**src/expand.rs:**
- Remove `hierarchies: vec![]` from all SemanticViewDefinition constructions (approximately 32 occurrences)

**tests/parse_proptest.rs:**
- Delete TEST-09 block: the `hierarchies_clause_no_panic` proptest (around line 856)
- Delete the `facts_and_hierarchies_combined_no_panic` proptest (around line 882)
- Delete the `empty_facts_hierarchies_clauses_valid` proptest (around line 906)
- Remove HIERARCHIES from any remaining DDL strings in other proptests
- Update comments referencing hierarchies

**tests/expand_proptest.rs:**
- Remove `hierarchies: vec![]` from all SemanticViewDefinition constructions

**Important:** After all removals, ensure no dead code warnings remain. The `Hierarchy` import in body_parser.rs must be fully removed. Check that `pub(crate)` on `parse_hierarchies_clause` does not leave orphan visibility.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && cargo test 2>&1 | tail -5</automated>
  </verify>
  <done>cargo test passes with zero hierarchy references in source code. No Hierarchy struct, no parse_hierarchies_clause, no validate_hierarchies, no hierarchies field on any struct. DESCRIBE output is 7 columns.</done>
</task>

<task type="auto">
  <name>Task 2: Update sqllogictests, examples, docs, and fuzz seeds</name>
  <files>test/sql/phase29_facts_hierarchies.test, test/sql/phase20_extended_ddl.test, test/sql/phase21_error_reporting.test, test/sql/phase25_keyword_body.test, test/sql/phase28_e2e.test, test/sql/phase30_derived_metrics.test, examples/advanced_features.py, README.md, fuzz/seeds/fuzz_ddl_parse/seed_hierarchies.txt, fuzz/seeds/fuzz_ddl_parse/seed_facts_and_hierarchies.txt</files>
  <action>
Update all non-Rust files to remove hierarchy references:

**test/sql/phase29_facts_hierarchies.test:**
- Rename file to `test/sql/phase29_facts.test` (it now only tests FACTS)
- Update file header comment: remove "and HIERARCHIES" / "hierarchy metadata"
- Test 1 (CREATE): Remove the `HIERARCHIES (geo AS (country, state, city))` clause from the CREATE statement (lines 51-53)
- Test 5 (DESCRIBE): Change `query TTTTTTTT` to `query TTTTTTT` (7 T's). Update the expected output line to remove the final `[{"levels":["country","state","city"],"name":"geo"}]` column. Update comment to say "7 columns" and remove "hierarchies"
- Test 8 (hierarchy error): DELETE the entire Test 8 block (lines 160-178) — hierarchy validation no longer exists
- Test 10 comment: Remove "(hierarchies optional)" — just say "FACTS without other optional clauses"
- Test 11 (HIERARCHIES without FACTS): DELETE the entire Test 11 block (lines 230-255) — no hierarchy-only view anymore
- Update test/sql/TEST_LIST: rename the entry from `phase29_facts_hierarchies.test` to `phase29_facts.test`

**test/sql/phase20_extended_ddl.test:**
- All DESCRIBE tests: Change `query TTTTTTTT` to `query TTTTTTT` (8 T's to 7 T's) at lines 199, 205, 296, 321
- Update comment at line 198: "7 columns: name, base_table, dimensions, metrics, filters, joins, facts" (remove "hierarchies")
- Update expected output lines: remove the trailing `\t[]` from each DESCRIBE result row (the 8th empty column). Each line currently ends with `[]\t[]` — change to just `[]` (remove the last tab + `[]`).

**test/sql/phase21_error_reporting.test:**
- Change `query TTTTTTTT` to `query TTTTTTT` at line 97
- Update comment at line 96: "7 columns" and remove "hierarchies"
- Update expected output: remove trailing `\t[]` from the DESCRIBE result row

**test/sql/phase25_keyword_body.test:**
- Change `query TTTTTTTT` to `query TTTTTTT` at line 84
- Update comment: "7 VARCHAR columns"
- Update expected output: remove trailing `\t[]` from the DESCRIBE result row

**test/sql/phase28_e2e.test:**
- Change `query TTTTTTTT` to `query TTTTTTT` at line 176
- Update comment at line 173: "7 columns" and remove "hierarchies"
- Update expected output: remove trailing `\t[]` from the DESCRIBE result row

**test/sql/phase30_derived_metrics.test:**
- Change `query TTTTTTTT` to `query TTTTTTT` at line 198
- Update expected output: remove trailing `\t[]` from the DESCRIBE result row

**examples/advanced_features.py:**
- Remove HIERARCHIES section entirely: the `HIERARCHIES (...)` clause in the CREATE statement, the "Section 3: HIERARCHIES" print/query block, and the comment about "Column index 7 contains hierarchies JSON"
- Renumber any remaining sections if needed
- Update the feature list comment at the top: remove "HIERARCHIES: drill-down path metadata"

**README.md:**
- Delete the "## Hierarchies (drill-down metadata)" section (around line 178-184)
- Remove HIERARCHIES from the "Full clause order" comment (around line 235)
- Remove HIERARCHIES from any syntax examples or feature lists

**Fuzz seeds:**
- Delete `fuzz/seeds/fuzz_ddl_parse/seed_hierarchies.txt` entirely
- Update `fuzz/seeds/fuzz_ddl_parse/seed_facts_and_hierarchies.txt`: remove the `HIERARCHIES (geo AS (country, state, city))` clause from the DDL string. Rename file to `seed_facts.txt`

**IMPORTANT for DESCRIBE expected output:** The current 8-column output has tab-separated values. Each row ends with the hierarchies column (usually `[]`). After removing the hierarchies column, the last column is now `facts` (also usually `[]`). Carefully trim only the final `\t[]` or `\t[{"levels":...}]` from each expected output line. Do NOT accidentally remove the facts column.
  </action>
  <verify>
    <automated>cd /Users/paul/Documents/Dev/Personal/duckdb-semantic-views && just test-all 2>&1 | tail -10</automated>
  </verify>
  <done>just test-all passes. No hierarchy references remain in tests, examples, docs, or fuzz seeds. DESCRIBE tests expect 7 columns. The renamed phase29_facts.test file runs successfully.</done>
</task>

</tasks>

<verification>
Run the full quality gate:
```bash
just test-all
```

Additionally verify no stale references:
```bash
grep -r "hierarch\|HIERARCHIES" src/ tests/ test/ examples/ README.md --include='*.rs' --include='*.py' --include='*.test' --include='*.md' --include='*.txt' | grep -v target/ | grep -v .planning/
```
This should return zero results (excluding any intentional "removed hierarchies" comments if added).
</verification>

<success_criteria>
- Zero references to Hierarchy/hierarchies/HIERARCHIES in src/, tests/, test/sql/, examples/, README.md, fuzz/seeds/
- cargo test passes (all Rust unit tests, proptests, doc tests)
- just test-sql passes (all sqllogictests with 7-column DESCRIBE)
- just test-ducklake-ci passes
- just test-all passes (full quality gate)
</success_criteria>

<output>
After completion, create `.planning/quick/260318-fzu-remove-hierarchies-syntax-no-backward-co/260318-fzu-SUMMARY.md`
</output>
