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

**Before pushing to main**, run the full CI mirror:

```bash
just ci
```

This adds linting (clippy pedantic + fmt + cargo-deny) and fuzz target compilation checks on top of `test-all`. The Rust toolchain version is pinned in `rust-toolchain.toml` and bumped automatically via Dependabot.

## Milestone Completion

At the end of every milestone, before tagging:

1. **Update CHANGELOG.md** — Add a new version section with user-facing feature descriptions. Group related commits into feature-level summaries (don't list individual commits). Follow Keep a Changelog format. Use ROADMAP.md phase descriptions and success criteria as the source, not raw git log.
2. **Add example file** — New Python example under `examples/` demoing the milestone's features.
3. **Bump version** — Update Cargo.toml + description.yml.

## Build

- `just build` — debug build (extension binary)
- `cargo test` — runs without the extension feature (in-memory DuckDB)
- `just test-sql` — requires a fresh `just build` to pick up code changes
