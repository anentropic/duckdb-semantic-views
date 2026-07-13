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
4. **Reclaim disk** — Run `just clean-stale` after the milestone tag is pushed. This uses `cargo-sweep` to evict target/ artifacts older than 14 days without invalidating the current build cache, so the next milestone's first build stays incremental. The DuckDB amalgamation produces ~30 GB of cumulative build-script output per toolchain rev across a milestone — left unchecked this fills the disk within a few milestones. Do **not** run `just clean` (full `cargo clean`) at milestone boundaries; the cold-rebuild cost (~10 min for the amalgamation) isn't worth it relative to `clean-stale`.

## Build

- `just build` — debug build (extension binary)
- `cargo test` — runs without the extension feature (in-memory DuckDB)
- `just test-sql` — requires a fresh `just build` to pick up code changes

### Offline amalgamation fallback (blocked GitHub — agent/sandbox sessions only)

`just build` → `make ensure_amalgamation` downloads the DuckDB amalgamation
(`cpp/include/duckdb.{hpp,cpp}`) from the DuckDB **GitHub release**. This is the
normal, canonical path — use it whenever GitHub is reachable (i.e. always, on a
normal local machine).

Some sandboxed agent sessions run behind an egress proxy that blocks
`github.com/duckdb/duckdb` (the release fetch returns HTTP 403), so `just build`
can't get the amalgamation. In that case only, regenerate the identical files
from GitHub-free hosts:

```bash
python3 scripts/fetch_amalgamation_offline.py
```

It pulls the DuckDB source from the PyPI sdist (`files.pythonhosted.org`) and
DuckDB's own amalgamation generator from jsDelivr (`cdn.jsdelivr.net`), writes
`cpp/include/duckdb.{hpp,cpp}` and caches them under `.amalgamation/<version>/`,
so the subsequent `just build` finds the correct version present and skips its
own (blocked) download. Output is byte-identical to the release amalgamation
except `DUCKDB_SOURCE_ID` (a placeholder — the real SHA isn't reachable without
GitHub).

**The placeholder source-id does NOT block loading the extension.** The debug
extension this produces uses the `C_STRUCT_UNSTABLE` ABI, which does not enforce
a source-id match, so it **loads and runs** — `just test-sql` / `just test-all`
(and `make test_debug`) pass end-to-end against it (empirically verified). So in
a blocked-GitHub session, after `python3 scripts/fetch_amalgamation_offline.py`,
run the **full** local gate — `just build` + `just test-sql` — as usual. Do
**not** assume the offline extension "can't load" and skip local sqllogictest in
favour of CI: it does load, and CLAUDE.md's quality gate expects sqllogictest to
run. (`just build` needs the `extension-ci-tools` git submodule checked out —
`git submodule update --init` — and `make configure` for the venv/platform
metadata, both of which `just setup` handles.)

Do **not** use the offline fetcher on a normal local machine — it's a fallback,
not a replacement for `just build`.

To lint the `extension`-gated FFI layer without any C++ build at all (no
amalgamation needed), use (see MAINTAINER.md):

```bash
SV_SKIP_CPP_BUILD=1 cargo clippy --no-default-features --features extension -- -D warnings
```

## Build/test command rules (non-negotiable)

These two rules have previously caused multi-hour agent stalls. They apply to every
command in this project's build/test surface: `just build`, `just test-sql`, `just test-all`,
`just ci`, `cargo build`, `cargo test`, `cargo nextest run`, `cargo fmt`, `cargo check`,
`cargo clippy`, `uv run test/integration/*.py`.

**Rule 1 — Never pipe long-running commands to bare `tail -N`.** The macOS pipe buffer fills,
`tail` waits for EOF that never arrives until the producer exits, and the run appears hung for
5-30 minutes. Always redirect to a file first, then tail the file:

```bash
cmd > /tmp/claude/x.log 2>&1
RC=$?
tail -100 /tmp/claude/x.log
```

This applies to ANY command above and any cargo/just/sqllogictest invocation that runs longer
than a few seconds.

**Rule 2 — Use `dangerouslyDisableSandbox: true` for the listed build/test commands when
needed.** The project's Makefile invokes `mktemp` which writes to `/var/folders/.../T/`
(macOS hardcoded), which the sandbox may block depending on session snapshot. If you see
`mktemp: mkstemp failed ... Operation not permitted`, use the sandbox bypass directly for
that command — no need to ask. The bypass is pre-approved for the build/test command list
above and ONLY those commands.

If a command not on the list needs the bypass, halt and ask first.

## Code editing rules

- Pre-commit hook runs `cargo fmt --check` + clippy. If a commit fails on fmt-check, run
  `cargo fmt`, re-stage, and retry. Never use `--no-verify`.
- New sqllogictest files must be added to `test/sql/TEST_LIST` or the runner will skip them.
- For `statement error` assertions in sqllogictest, use the block form (`---- separator` +
  substring), not inline regex — the runner does not support inline form.
