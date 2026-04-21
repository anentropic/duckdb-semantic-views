# Editor Report

**Generated:** 2026-04-21
**Files reviewed:** 16
**Changes made:** 24
  - BLOCKING: 6
  - SUGGESTION: 12
  - NITPICK: 6

## Summary

The documentation is well-written and free of AI patterns. The primary changes were stripping internal HTML comments from all 16 pages and adding cross-reference links to unlinked inline code mentions of API symbols. No terminology inconsistencies, type blur, or AI writing patterns were found.

---

## docs/how-to/materializations.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| How Materializations Work | Unlinked `semantic_view()` code mention in prose | Linked to `:ref:\`semantic_view() <ref-semantic-view-function>\`` |
| Troubleshooting | Unlinked `explain_semantic_view()` code mention in prose | Linked to `:ref:\`explain_semantic_view() <ref-explain-semantic-view>\`` |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (31 lines) |

---

## docs/how-to/yaml-definitions.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Export with READ_YAML_FROM_SEMANTIC_VIEW | Unlinked `READ_YAML_FROM_SEMANTIC_VIEW()` code mention (first occurrence in prose) | Linked to `:ref:\`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>\`` |
| Troubleshooting | Unlinked `READ_YAML_FROM_SEMANTIC_VIEW()` code mention in error description | Linked to `:ref:\`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>\`` |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (29 lines) |

---

## docs/reference/show-semantic-materializations.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (26 lines) |

---

## docs/reference/read-yaml-from-semantic-view.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (24 lines) |

---

## docs/explanation/databricks-comparison.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| Concept Mapping table | Unlinked `READ_YAML_FROM_SEMANTIC_VIEW()` in YAML definitions row | Linked to `:ref:\`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>\`` |
| Features in DuckDB table | Unlinked `explain_semantic_view()` and `READ_YAML_FROM_SEMANTIC_VIEW()` in feature rows | Linked both to their respective `:ref:` targets |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (27 lines) |

---

## docs/reference/create-semantic-view.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (35 lines) |

---

## docs/reference/describe-semantic-view.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (25 lines) |

---

## docs/reference/explain-semantic-view-function.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (25 lines) |

---

## docs/reference/get-ddl.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (21 lines) |

---

## docs/reference/error-messages.rst

### BLOCKING

| Section | Description | Fix |
|---------|-------------|-----|
| YAML Errors intro | Unlinked `READ_YAML_FROM_SEMANTIC_VIEW()` code mention in section intro | Linked to `:ref:\`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>\`` |

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (27 lines) |

---

## docs/explanation/semantic-views-vs-regular-views.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Materialization Support | Unlinked `semantic_view()` in final paragraph linked | Linked to `:ref:\`semantic_view() <ref-semantic-view-function>\`` |
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (25 lines) |

---

## docs/explanation/snowflake-comparison.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| A Note on Snowflake's YAML Spec | Unlinked `READ_YAML_FROM_SEMANTIC_VIEW()` in YAML section linked | Linked to `:ref:\`READ_YAML_FROM_SEMANTIC_VIEW() <ref-read-yaml>\`` |
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (19 lines) |

---

## docs/reference/index.rst

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (12 lines) |

---

## docs/how-to/index.rst

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (12 lines) |

---

## docs/index.rst

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (6 lines) |

---

## docs/explanation/index.rst

### NITPICK

| Section | Description | Fix |
|---------|-------------|-----|
| (whole page) | Internal HTML comment block stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (10 lines) |

---

## Terminology Changes

| Term | Before | After | Authority |
|------|--------|-------|-----------|
| (none) | -- | -- | No terminology inconsistencies found across all doc files |

**New terms added to terminology.yaml:**

| Term | Source |
|------|--------|
| `READ_YAML_FROM_SEMANTIC_VIEW()` | scalar function in src/ddl/read_yaml.rs |
| `Materialization` | struct in src/model.rs |
| `Databricks` | proper noun (new comparison page) |
| `Delta Lake` | proper noun (Databricks comparison) |
| `Unity Catalog` | proper noun (Databricks comparison) |
| `semi-additive metric` | project term |
| `window metric` | project term |
| `materialization` | project term |
