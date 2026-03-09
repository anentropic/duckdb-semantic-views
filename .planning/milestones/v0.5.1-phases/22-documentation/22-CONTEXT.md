# Phase 22: Documentation - Context

**Gathered:** 2026-03-09
**Status:** Ready for planning
**Source:** Direct user guidance

<domain>
## Phase Boundary

Update the README to replace function-based syntax examples with the new native DDL syntax and show all new DDL verbs (CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW).

</domain>

<decisions>
## Implementation Decisions

### Scope
- Keep it simple — match the current level of detail in the README
- Replace existing function syntax examples with DDL equivalents
- Show all new DDL verbs
- Do NOT add comprehensive documentation — that comes in a later phase

### Style
- Copy the tone and detail level already in the README
- No over-documentation

### Claude's Discretion
- Exact section ordering and headings
- Whether to keep function syntax as an alternative or replace entirely
- Wording of examples

</decisions>

<specifics>
## Specific Ideas

- Replace `create_semantic_view()` examples with `CREATE SEMANTIC VIEW` equivalents
- Add examples for all DDL verbs: CREATE, CREATE OR REPLACE, CREATE IF NOT EXISTS, DROP, DROP IF EXISTS, DESCRIBE, SHOW
- One worked lifecycle example (create, query, describe, drop)

</specifics>

<deferred>
## Deferred Ideas

- Comprehensive documentation (dedicated future phase)

</deferred>

---

*Phase: 22-documentation*
*Context gathered: 2026-03-09 via direct user guidance*
