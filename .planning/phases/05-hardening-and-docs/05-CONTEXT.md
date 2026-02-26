# Phase 5: Hardening and Docs - Context

**Gathered:** 2026-02-26
**Status:** Ready for planning

<domain>
## Phase Boundary

Make the extension resilient against malformed inputs at the FFI boundary and document it well enough for the maintainer (a Python expert new to Rust and DuckDB extensions) to build, test, fuzz, extend, and publish without needing to ask for help. Covers TEST-05 (fuzz targets) and DOCS-01 (MAINTAINER.md).

</domain>

<decisions>
## Implementation Decisions

### Fuzz target scope
- Three separate fuzz targets, all equally prioritized:
  1. **JSON definition parsing** — fuzz `define_semantic_view` with arbitrary strings at the FFI boundary
  2. **SQL generation** — fuzz `expand()` with arbitrary `SemanticViewDefinition` structs
  3. **Query-time name arrays** — fuzz dimension/metric name strings against a valid definition (catches injection via column names)
- Validation: no panics AND output SQL must parse successfully (not just crash-free)
- Seed corpus from existing test JSON definitions — fuzzer mutates from known-good inputs

### MAINTAINER.md audience and tone
- Primary reader: the project maintainer — expert in Python, knows the semantic views domain, but new to Rust toolchains and DuckDB extension development
- Brief inline Rust concept explanations where they appear (e.g., "cargo-fuzz generates random inputs to find crashes — like Python's hypothesis but for binary data")
- Explain the "why" behind troubleshooting fixes, not just the commands — build understanding of the Rust/DuckDB build system
- DuckDB install context: user primarily uses DuckDB through Python (`pip install duckdb`)

### Doc coverage depth
- All sections equally detailed: dev setup, build, test, LOAD, version pin update, fuzzer, publishing
- Include a brief architecture overview mapping source tree to concepts
- Include worked examples for common extension tasks (e.g., adding a new DDL function, adding a new metric type)
- Version pin update section: just the steps (no deep ABI explainer)

### Fuzzer runtime and CI integration
- Both local and nightly CI: developer runs locally for deep fuzzing, scheduled CI runs nightly
- Nightly CI: 5 minutes per fuzz target (15 minutes total for 3 targets)
- On crash: CI opens a GitHub issue with crash artifact and reproduction steps
- Corpus committed to repo under `fuzz/corpus/` — shared between local and CI runs
- CI auto-commits new corpus entries via PR after each nightly run (corpus files are tiny, deduplicated by coverage)
- Periodic `cargo fuzz cmin` to minimize corpus if it grows

### Claude's Discretion
- Exact fuzz target implementation details (harness structure, arbitrary trait implementations)
- SQL validity check mechanism (DuckDB parser vs basic syntax check)
- Troubleshooting section: which errors to include (pick the most common ones from the build system)
- Architecture overview structure and level of detail
- Worked example selection (which extension tasks are most instructive)

</decisions>

<specifics>
## Specific Ideas

- Rust concept explanations should feel like footnotes, not lectures — one-liner analogies to Python where possible
- MAINTAINER.md should be the single doc a contributor needs — no "see also" chains to external docs for essential workflows
- Fuzz corpus growth from CI is expected to stay small (few hundred files, few MB) based on codebase size

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-hardening-and-docs*
*Context gathered: 2026-02-26*
