---
status: diagnosed
phase: 21-error-location-reporting
source: 21-01-SUMMARY.md, 21-02-SUMMARY.md
started: 2026-03-09T14:00:00Z
updated: 2026-03-09T14:15:00Z
---

## Current Test

[testing complete]

## Tests

### 1. Missing Clause Error Hint
expected: Run DDL missing a required clause (e.g., TABLES). DuckDB returns an error message hinting at the missing clause.
result: pass

### 2. Clause Keyword Typo Suggestion
expected: Run DDL with a typo in a clause keyword (e.g., "DIMESIONS" instead of "DIMENSIONS"). DuckDB returns a "Did you mean DIMENSIONS?" style suggestion.
result: issue
reported: "Wrong error; reports missing 'tables' even though TABLES is present, and no typo suggestion for DIMESIONS"
severity: major

### 3. Empty Body Error
expected: Run DDL with empty parentheses. DuckDB returns a clear error about the empty body.
result: pass

### 4. Unbalanced Brackets Error
expected: Run DDL with mismatched parentheses. DuckDB returns a positioned error pointing to the bracket problem.
result: pass

### 5. Near-Miss DDL Prefix Suggestion
expected: Type a near-miss prefix like "CREAT SEMANTIC VIEW". DuckDB returns a "Did you mean CREATE SEMANTIC VIEW?" suggestion.
result: pass

### 6. Non-Interference with Valid DDL
expected: Valid CREATE SEMANTIC VIEW, DESCRIBE, DROP, and normal SQL all work normally — no false error triggers.
result: issue
reported: "Valid CREATE SEMANTIC VIEW fails with false 'Missing required clause tables' error"
severity: blocker

## Summary

total: 6
passed: 4
issues: 2
pending: 0
skipped: 0

## Gaps

- truth: "Clause keyword typo produces 'Did you mean' suggestion"
  status: failed
  reason: "User reported: Wrong error; reports missing 'tables' even though TABLES is present, and no typo suggestion for DIMESIONS"
  severity: major
  test: 2
  root_cause: "scan_clause_keywords() at src/parse.rs:420 only recognizes clause keywords followed by ':=', not 'KEYWORD (...)' syntax. When TABLES is followed by '(' instead of ':=', it is silently skipped. found_clauses returns empty, causing false 'missing tables' error."
  artifacts:
    - path: "src/parse.rs"
      issue: "scan_clause_keywords line 420: starts_with(':=') gate excludes KEYWORD (...) syntax"
  missing:
    - "scan_clause_keywords must also recognize KEYWORD followed by '(' as a clause keyword candidate"
    - "Typo suggestion logic must fire for unrecognized words followed by '(' not just ':='"
  debug_session: ".planning/debug/validate-clauses-tables-not-detected.md"

- truth: "Valid CREATE SEMANTIC VIEW executes successfully without false errors"
  status: failed
  reason: "User reported: Valid CREATE SEMANTIC VIEW fails with false 'Missing required clause tables' error"
  severity: blocker
  test: 6
  root_cause: "Same root cause as test 2: scan_clause_keywords() only recognizes ':=' syntax. Valid DDL using 'KEYWORD (...)' syntax has all clauses silently skipped, returning empty found_clauses, which triggers the missing-tables error."
  artifacts:
    - path: "src/parse.rs"
      issue: "scan_clause_keywords line 420: starts_with(':=') gate excludes KEYWORD (...) syntax"
  missing:
    - "scan_clause_keywords must recognize both ':=' and '(' as clause-keyword delimiters"
    - "Unit tests must cover KEYWORD (...) syntax, not just ':=' syntax"
  debug_session: ".planning/debug/validate-clauses-tables-not-detected.md"
