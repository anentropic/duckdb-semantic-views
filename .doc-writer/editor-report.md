# Editor Report

**Generated:** 2026-04-13
**Files reviewed:** 4 (changed pages) + 28 (terminology scan across all docs)
**Changes made:** 5
  - BLOCKING: 0
  - SUGGESTION: 1
  - NITPICK: 0

## Summary

The four changed pages are well-written with consistent terminology, correct cross-references, and no structural type blur. One minor AI-writing pattern was fixed (significance inflation in error-messages.rst). Internal notes were stripped from all four files. All accuracy claims verified against source code.

---

## docs/how-to/window-metrics.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Post-processing | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (38 lines) |

No terminology, type blur, humanizer, or cross-reference issues found. Page is a clean how-to with consistent structure matching other how-to guides.

---

## docs/reference/create-semantic-view.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Post-processing | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (18 lines) |

No terminology, type blur, humanizer, or cross-reference issues found. Page follows the SQL reference pattern consistently.

---

## docs/reference/show-semantic-dimensions-for-metric.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Post-processing | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (18 lines) |

No terminology, type blur, humanizer, or cross-reference issues found.

---

## docs/reference/error-messages.rst

### SUGGESTION

| Section | Description | Fix |
|---------|-------------|-----|
| Window and aggregate metric mixing | AI vocabulary: "fundamentally" removed from "These produce fundamentally different result shapes" | Changed to "These produce different result shapes (row-level vs. grouped)" |
| Post-processing | Internal notes stripped | Removed `<!-- INTERNAL NOTES FOR EDITOR -->` block (20 lines) |

No terminology, type blur, or cross-reference issues found. All existing `:ref:` links verified as valid.

---

## Terminology Changes

No terminology changes were needed. All terms across all 28+ documentation files are consistent with the canonical forms in `terminology.yaml` and match source code symbol names.

| Term | Before | After | Authority |
|------|--------|-------|-----------|
| (none) | -- | -- | -- |
