# Editor Report

**Generated:** 2026-04-02
**Files reviewed:** 6 (new pages) + 14 (existing pages scanned for terminology)
**Changes made:** 9
  - BLOCKING: 3
  - SUGGESTION: 0
  - NITPICK: 6

## Summary

The 6 new reference pages are well-written, accurate against source code, and maintain consistent terminology with the rest of the documentation. No AI-writing patterns detected and no type blur found. The only substantive issues are accuracy mismatches in 3 existing pages whose output examples and descriptions were not updated to reflect the v0.5.5 schema changes documented by these new pages.

---

## docs/reference/show-semantic-views.rst

### BLOCKING

*No blocking issues.*

### SUGGESTION

*No suggestions.*

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Full file | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (lines 201-223) |

---

## docs/reference/show-semantic-dimensions.rst

### BLOCKING

*No blocking issues.*

### SUGGESTION

*No suggestions.*

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Full file | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (lines 262-285) |

---

## docs/reference/show-semantic-metrics.rst

### BLOCKING

*No blocking issues.*

### SUGGESTION

*No suggestions.*

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Full file | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (lines 245-268) |

---

## docs/reference/show-semantic-facts.rst

### BLOCKING

*No blocking issues.*

### SUGGESTION

*No suggestions.*

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Full file | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (lines 275-299) |

---

## docs/reference/show-semantic-dimensions-for-metric.rst

### BLOCKING

*No blocking issues.*

### SUGGESTION

*No suggestions.*

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Full file | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (lines 305-328) |

---

## docs/reference/describe-semantic-view.rst

### BLOCKING

*No blocking issues.*

### SUGGESTION

*No suggestions.*

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| Full file | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (lines 418-446) |

---

## docs/tutorials/getting-started.rst (NOT EDITED -- existing page)

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Query the Semantic View (line 110-121) | Accuracy mismatch: SHOW SEMANTIC VIEWS output shows old 2-column format (`name`, `base_table`). As of v0.5.5, the output has 5 columns: `created_on`, `name`, `kind`, `database_name`, `schema_name`. The example output and surrounding prose need updating. | Author should update the SHOW SEMANTIC VIEWS example output and text to reflect the new 5-column schema. |

---

## docs/tutorials/multi-table.rst (NOT EDITED -- existing page)

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Describe the View (line 193-195) | Accuracy mismatch: prose says "The output shows the view name, base table, and JSON arrays for dimensions, metrics, joins, and facts." This describes the pre-0.5.5 single-row JSON format. DESCRIBE SEMANTIC VIEW now returns a multi-row property-per-row format with 5 VARCHAR columns (`object_kind`, `object_name`, `parent_entity`, `property`, `property_value`). | Author should update the description and consider adding an example output showing the new property-per-row format. |

---

## docs/reference/error-messages.rst (NOT EDITED -- existing page)

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Empty request (line 193-194) | Accuracy mismatch: error message shows `Run FROM describe_semantic_view('<name>') to see available dimensions and metrics.` but the source code (`src/query/error.rs` line 61) now says `Run DESCRIBE SEMANTIC VIEW {view_name} to see available dimensions and metrics.` (without `FROM` prefix, using DDL syntax instead of function syntax). | Author should update the error message text to match the current source code. |

---

## Terminology Changes

No terminology changes were needed. All 6 new pages use terms consistent with the existing terminology map and the rest of the documentation.

| Term | Before | After | Authority |
|------|--------|-------|-----------|
| *(none)* | | | |
