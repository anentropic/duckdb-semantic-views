# DuckDB Semantic Views — Project Instructions

If in doubt about SQL syntax or behaviour refer to what Snowflake semantic views does.

## Quality Gate

**All phases must pass the full test suite before verification can be marked complete.**

The verification command is:

```bash
just test-all
```

This runs: Rust unit tests, property-based tests, SQL logic tests (sqllogictest), and DuckLake CI tests.

Individual test commands:
- `cargo test` — Rust unit + proptest + doc tests
- `just test-sql` — SQL logic tests via sqllogictest runner (requires `just build` first)
- `just test-ducklake-ci` — DuckLake integration tests

A phase verification that only runs `cargo test` is **incomplete** — sqllogictest covers integration paths that Rust tests do not (e.g., type dispatch through the full extension load → DDL → query pipeline).

## Build

- `just build` — debug build (extension binary)
- `cargo test` — runs without the extension feature (in-memory DuckDB)
- `just test-sql` — requires a fresh `just build` to pick up code changes
