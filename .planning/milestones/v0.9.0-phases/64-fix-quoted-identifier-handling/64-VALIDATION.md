---
phase: 64
slug: fix-quoted-identifier-handling
status: approved
nyquist_compliant: true
wave_0_complete: false
created: 2026-05-17
---

# Phase 64 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework (primary)** | `cargo test` — Rust unit tests + proptest (proptest 1.x dev-dep, already in `Cargo.toml`) |
| **Framework (secondary)** | sqllogictest via `just test-sql` (requires `just build` first) |
| **Tertiary** | `cargo fuzz build fuzz_ddl_parse` — compile-only fuzz target check (gated by `just ci`) |
| **Config file** | `Cargo.toml` (unit + proptest), `test/sql/TEST_LIST` (sqllogictest registration), `fuzz/Cargo.toml` (fuzz targets) |
| **Quick run command** | `cargo test --lib ident::` |
| **Full suite command** | `just test-all` (cargo test + sqllogictest + DuckLake CI) |
| **Pre-push gate** | `just ci` (adds clippy pedantic + fmt + cargo-deny + fuzz target compile) |
| **Estimated runtime** | ~60-120 s for `just test-all`; ~5 s for `cargo test --lib ident::`; ~30 s for `just test-sql phase64_quoted_idents` (includes build) |

---

## Sampling Rate

- **After every task commit:** `cargo test --lib ident::` (< 1 s — keep feedback tight)
- **After every plan wave:**
  - Wave 1 (64-01 complete): `cargo test --lib ident::` + `cargo build --lib`
  - Wave 2 (64-02 + 64-03 complete): `cargo test --lib` (full unit suite)
  - Wave 3 (64-04 in progress): `just build && just test-sql phase64_quoted_idents`
- **Before `/gsd-verify-work`:** `just test-all` AND `just ci` both green
- **Max feedback latency:** ≤ 5 s for the quick command; ≤ 120 s for the full gate

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 64-01-01 | 01 | 1 | QID-01, QID-02, QID-07 | — | N/A (parser hardening only; no auth/PII surface) | unit | `cargo test --lib ident::tests` | ✅ (Wave 0 creates `src/ident.rs`) | ⬜ pending |
| 64-01-02 | 01 | 1 | QID-07 | — | N/A | property (proptest) | `cargo test --lib ident::tests::proptests` | ✅ | ⬜ pending |
| 64-02-01 | 02 | 2 | QID-01, QID-02, QID-03 | — | Rejects malformed identifier strings via `Err` return from `normalize_view_name` (no panic, no SQL injection — names normalised before catalog INSERT) | unit | `cargo test --lib parse::` | ✅ (`src/parse.rs` already exists) | ⬜ pending |
| 64-02-02 | 02 | 2 | QID-03, QID-06 | — | Runtime `bind.get_parameter(0)` normalised before catalog lookup; malformed input surfaces clean error rather than raw token leak | unit + manual trace | `cargo test --lib` | ✅ (`src/parse.rs`, `src/query/table_function.rs`) | ⬜ pending |
| 64-03-01 | 03 | 2 | QID-04 | — | Expansion no longer emits arbitrarily nested quotes that could confuse downstream SQL parsers | unit | `cargo test --lib expand::resolution::tests` | ✅ (`src/expand/resolution.rs`) | ⬜ pending |
| 64-03-02 | 03 | 2 | QID-04 | — | Structural part-count check replaces brittle `.contains('.')` substring test | unit | `cargo test --lib expand::` | ✅ | ⬜ pending |
| 64-04-01 | 04 | 3 | QID-01..QID-06 | — | End-to-end CREATE/lookup/error-message round-trip exercised via the full extension load → parser_override → catalog → expand pipeline | sqllogictest | `just build && just test-sql phase64_quoted_idents` | ❌ W0 (`test/sql/phase64_quoted_idents.test` created by this task) | ⬜ pending |
| 64-04-02 | 04 | 3 | QID-01, QID-02, QID-03 | — | Fuzz target keeps quoted-identifier paths warm; never panics on quoted inputs | fuzz-compile + regression | `cargo build --manifest-path fuzz/Cargo.toml --release` and `cargo test --test quoted_idents_regression` (if workspace test path used) | ❌ W0 (seed files / regression test created by this task) | ⬜ pending |
| 64-04-03 | 04 | 3 | QID-01..QID-07 | — | Documentation + traceability — no runtime behaviour change | docs grep + full quality gate | `just test-all && just ci` | ✅ | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

Threat-model column is "—" throughout: Phase 64 is a parser-normalisation bugfix on a path that already runs inside a transactional DDL guard, with no new external input surface, no auth changes, and no PII handling. The relevant secure-behavior column captures the parser-hardening intent (no panics, no raw-token leak into catalog rows or error messages, bounded recursion in the identifier state machine).

---

## Wave 0 Requirements

Wave 0 in this phase means "scaffolding tasks that downstream waves verify against." Two scaffolding files must exist before later tasks' `<verify>` commands can pass:

- [ ] `src/ident.rs` — created by 64-01 Task 1 (Wave 1). Downstream tasks in 64-02 / 64-03 verify against this module's public API.
- [ ] `test/sql/phase64_quoted_idents.test` — created by 64-04 Task 1 (Wave 3). This fixture is the end-to-end acceptance file.
- [ ] `test/sql/TEST_LIST` entry for the above — created by 64-04 Task 1 (Wave 3). Without it the runner silently skips the fixture (Phase 63 Plan 02 lesson).

Notes:
- `src/lib.rs` exists; 64-01 only inserts a one-line `pub mod ident;` declaration.
- `src/parse.rs`, `src/query/table_function.rs`, `src/expand/resolution.rs` all exist; 64-02 and 64-03 modify them in place.
- `fuzz/fuzz_targets/fuzz_ddl_parse.rs` exists; 64-04 adds a comment header and corpus seeds.
- `cargo`, `just`, `cargo fuzz` toolchains are already installed (rust-toolchain.toml pinned; Phase 63 used the same gate).

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| — | — | All phase behaviors have automated verification via `cargo test`, `just test-sql`, `just test-all`, and `just ci`. | — |

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify (every task in 64-01..64-04 has an explicit `<automated>` command)
- [x] Wave 0 covers all MISSING references (`src/ident.rs`, `test/sql/phase64_quoted_idents.test`, TEST_LIST entry)
- [x] No watch-mode flags (all commands are one-shot)
- [x] Feedback latency < 120 s for full gate; < 5 s for quick command
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-05-17
