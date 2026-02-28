# Phase 1: Scaffold - Context

**Gathered:** 2026-02-24
**Status:** Ready for planning

<domain>
## Phase Boundary

A loadable DuckDB extension with CI/CD passing on all 5 target platforms, code quality enforced, and all architectural decisions locked before any business logic is written. This phase produces infrastructure only — no semantic layer functionality.

</domain>

<decisions>
## Implementation Decisions

### CI platform matrix
- Branch model: `main` / `release/vX.Y` / `feature/*` (git flow)
- Feature branches: Linux x86_64 only — fast feedback per PR
- `main` and `release/*` branches: full 5-platform matrix (Linux x86_64, Linux arm64, macOS x86_64, macOS arm64, Windows x86_64)
- All 5 platforms must pass to merge into main or release branches

### Scheduled DuckDB version monitoring
- Weekly cron job polls GitHub API for the latest DuckDB release tag
- On new release + build passes: opens an auto-bump PR with the version update
- On new release + build fails: opens a breakage PR with failure log and `@copilot please update the DuckDB version pin and fix any compilation errors`
- Both success and failure scenarios trigger a PR — version stays current automatically

### Developer experience tooling
- Task runner: `just` (Justfile) for common commands — `just build`, `just test`, `just lint`, `just setup`
- Test runner: `cargo-nextest` (faster parallel execution, better output) replaces `cargo test`
- Pre-commit hooks via `cargo-husky`: runs `rustfmt` and `clippy` before each commit
- `just setup` downloads the pinned DuckDB binary locally — ensures local tests use the same version as CI

### Code quality gates
- `clippy` pedantic lints + `deny(warnings)` — zero tolerance, all warnings are errors
- `rustfmt` enforced on all code
- `cargo-deny` with `deny.toml` covering: disallowed licenses and known security advisories
- Code coverage gated at 80% minimum — CI fails if coverage drops below threshold
- `CHANGELOG.md` maintained from day 1 (Keep a Changelog format)

### Claude's Discretion
- Exact clippy lint suppressions for known-noisy pedantic rules (e.g., `module_name_repetitions`)
- Coverage tool selection (llvm-cov vs tarpaulin)
- Specific `deny.toml` allowed license list
- Justfile command names and structure
- Pre-commit hook implementation details

</decisions>

<specifics>
## Specific Ideas

- The @copilot PR mention is intentional — use it to request automated fix attempts on version breakage
- `just setup` should be the single command a new contributor runs to get a working dev environment

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 01-scaffold*
*Context gathered: 2026-02-24*
