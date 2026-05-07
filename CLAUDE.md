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

1. **Update CHANGELOG.md** — Add a new version section with user-facing feature descriptions. Group related commits into feature-level summaries (don't list individual commits). Use ROADMAP.md phase descriptions and success criteria as the source, not raw git log.
   - **Format**: Keep a Changelog 1.1.0. The only allowed `###` subheadings under a version are `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, `Security`. `Known limitations` is also permitted as a final subheading when a release ships with documented constraints. **Do not** introduce ad-hoc subheadings for internal phases ("Phase 62 — ..."), workstreams, or chronology — fold those bullets into the standard categories.
   - **Unreleased section**: keep an `## [Unreleased]` section at the top above the most recent tagged version. Between milestones it can read `_No unreleased changes yet._`; in-flight changes on `main` that aren't yet folded into a milestone version go here. The matching `[Unreleased]: ...compare/<latest-tag>...HEAD` link reference at the bottom must point at the latest tag.
   - **In-version churn**: if a feature was added and reverted within the same unreleased version, do not list it in `Added`. Only list what actually shipped at tag time. Likewise, do not include strikethrough "resolved later in the same version" entries.
   - **Audience**: this file is also rendered verbatim as the docs site Release Notes page (`docs/changelog.md` includes it via MyST). Avoid GSD/phase-internal vocabulary in user-facing bullets; if implementation detail belongs anywhere it's inline within the relevant `Added`/`Changed`/`Fixed` bullet, not as its own subhead.
2. **Add example file** — New Python example under `examples/` demoing the milestone's features.
3. **Bump version** — Update Cargo.toml + description.yml.

## Build

- `just build` — debug build (extension binary)
- `cargo test` — runs without the extension feature (in-memory DuckDB)
- `just test-sql` — requires a fresh `just build` to pick up code changes
