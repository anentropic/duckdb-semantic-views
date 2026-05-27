---
phase: 68
slug: pre-tag-cleanup-phase-67-review-pr-35-code-review-follow-ups
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-27
---

# Phase 68 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Derived from `68-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework — Rust unit** | `cargo test` (workspace default) |
| **Framework — sqllogictest** | `sqllogictest-bin` runner via `just test-sql` (requires `just build` first to materialize the loadable extension) |
| **Framework — integration** | `pytest` (existing) — `test/integration/test_adbc_queries.py` for A2 |
| **Config file** | `Cargo.toml` (workspace), `test/sql/TEST_LIST` (sqllogictest registry) |
| **Quick run command** | `cargo test --lib body_parser` (hot loop on parser changes); `cargo test --test registration_error_surfaces` for C1/C2 |
| **Full suite command** | `just test-all` (Rust unit + proptest + sqllogictest + DuckLake CI + ADBC) |
| **CI mirror** | `just ci` (adds clippy pedantic + fmt + cargo-deny + fuzz target compile) — required before push to main |
| **Estimated runtime** | `cargo test --lib`: ~30s warm cache · `just test-all`: ~3–5 min warm cache · `just ci`: ~6–8 min warm cache |

---

## Sampling Rate

- **After every task commit:** `cargo test --lib body_parser` (~30s); for B1/B2 also run `just test-sql <new_fixture>` once the fixture lands
- **After every plan wave:** `just test-all` (full suite)
- **Before `/gsd-verify-phase`:** `just test-all` green
- **Before push to main (milestone close):** `just ci` green
- **Max feedback latency:** ~30s per-task, ~5 min per-wave

---

## Per-Task Verification Map

Phase 68 has no REQUIREMENTS.md mappings — all 13 items are review-derived hygiene. Verification rows are indexed by SCOPE.md item ID. Threat refs map to the phase's `<threat_model>` block (parser input handling).

| Item | Plan | Wave | Behavior | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|------|------|------|----------|------------|-----------------|-----------|-------------------|-------------|--------|
| A1 | 68-01 | 1 | Bare reserved keyword after AS surfaces structured ParseError | T-68-01 (malformed DDL input) | Reject `PRIMARY|UNIQUE|FOREIGN|REFERENCES|NOT` as bare table name with pre-Phase-67 error message | unit | `cargo test --lib test_parse_single_table_entry_reserved_keyword_after_as` | ❌ W0 | ⬜ pending |
| A1 | 68-01 | 1 | Same, end-to-end via DDL | T-68-01 | (same) | sqllogictest | `just test-sql phase67_quoted_source_tables` | Extend existing | ⬜ pending |
| A2 | 68-01 | 1 | ADBC ATTACH path interpolation escapes `'` | T-68-02 (SQL string injection defense-in-depth) | `.replace("'", "''")` parity with line 100 | integration | `uv run pytest test/integration/test_adbc_queries.py::test_adbc_queries_basic` | ✅ | ⬜ pending |
| A3 | 68-01 | 1 | Dead loop arm removed; happy path unchanged | — | (no security impact) | (covered by existing) | `cargo test --lib body_parser` (existing TECH-DEBT #24 tests) | ✅ | ⬜ pending |
| A4 | 68-01 | 1 | Unterminated quoted identifier rejected at parse time | T-68-01 | Reject malformed input with `ParseError`, no silent acceptance | unit | `cargo test --lib test_parse_single_table_entry_unterminated_quote` | ❌ W0 | ⬜ pending |
| A5 | 68-01 | 1 | Mixed bare/quoted dot-qualified name parses | — | (no security impact) | unit + sqllogictest | `cargo test --lib test_parse_single_table_entry_mixed_quoted_and_bare` + `just test-sql phase67_quoted_source_tables` | Extend existing | ⬜ pending |
| A6 | 68-01 | 1 | Fixture cleanup is complete | — | (hygiene) | (none — fixture must still pass) | `just test-sql phase67_quoted_source_tables` | ✅ | ⬜ pending |
| A7 | 68-01 | 1 | `find_primary_key` word-boundary matches `find_unique` | — | (no security impact; alignment fix) | unit (optional) | `cargo test --lib find_primary_key_word_boundary` | ❌ Optional | ⬜ pending |
| C1 | 68-02 | 1 | UTF-8 char boundary safe in error formatter | T-68-03 (panic via malformed UTF-8 input on assertion failure path) | `body.get(..400).unwrap_or(body)` returns safely on non-UTF-8 boundary | (code-review verified) | `cargo test --test registration_error_surfaces` (existing — must still pass) | ✅ | ⬜ pending |
| C2 | 68-02 | 1 | Transmute needle catches bare + turbofish | T-68-04 (FFI safety invariant — body must not contain `transmute`) | Looser needle still catches both forms; test continues passing on absence of `transmute` | (existing test stays green) | `cargo test --test registration_error_surfaces` | ✅ | ⬜ pending |
| C3 | 68-02 | 1 | Dead fixture deleted, gating test still passes | — | (hygiene) | (deletion only) | `just test-sql phase651_yaml_filesystem_access_gating` | ✅ | ⬜ pending |
| B1 | 68-03 | 2 | NAB clause accepts quoted identifier with literal whitespace | T-68-05 (quoted-identifier handling in DDL clauses) | Identifier-aware tokenisation preserves quoted segments intact (no `split_whitespace` split) | unit + sqllogictest | `cargo test --lib test_parse_non_additive_dims_quoted_identifier_with_whitespace` + `just test-sql phase68_quoted_idents_non_additive` | ❌ W0 (new file) | ⬜ pending |
| B1 | 68-03 | 2 | NAB clause accepts dotted path `table.col` | — | (contract extension per D-08) | unit + sqllogictest | (same) | ❌ W0 | ⬜ pending |
| B2 | 68-03 | 2 | OVER ORDER BY accepts quoted identifier with literal whitespace | T-68-05 | (same as B1) | unit + sqllogictest | `cargo test --lib test_parse_window_spec_quoted_order_by` + `just test-sql phase68_quoted_idents_window` | ❌ W0 (new file) | ⬜ pending |
| B2 | 68-03 | 2 | OVER ORDER BY accepts dotted path `table.col` | — | (contract extension per D-08) | unit + sqllogictest | (same) | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `test/sql/phase68_quoted_idents_non_additive.test` — new sqllogictest fixture (B1, per D-09)
- [ ] `test/sql/phase68_quoted_idents_window.test` — new sqllogictest fixture (B2, per D-09)
- [ ] `test/sql/TEST_LIST` — register the two new fixtures (CLAUDE.md hard rule; runner silently skips unlisted files — Phase 63 Plan 02 documented this gate)
- [ ] New Rust unit tests in `src/body_parser.rs::tests` — 5+ new tests covering A1, A4, A5, B1, B2 (one balanced-quote helper test for A4)
- [ ] Optional new sibling Rust test for A7 word-boundary alignment (planner discretion)

---

## Manual-Only Verifications

| Behavior | Item | Why Manual | Test Instructions |
|----------|------|------------|-------------------|
| C1 panic path | C1 | Behavioral test infeasible — the char-boundary panic only fires from inside the assertion's error-formatting path on a non-UTF-8 body, which itself only fires when the assertion has already failed. Triggering it requires injecting a non-UTF-8 byte sequence into the FFI body, which is exactly what the test guards against. Code review confirms `body.get(..400).unwrap_or(body)` is panic-free. | Inspect `tests/registration_error_surfaces.rs:136` diff — confirm slicing operator replaced; existing test must still pass. |
| C2 turbofish needle | C2 | The test asserts an FFI safety invariant (no `transmute` in body); needle change is verified by inspection because both bare and turbofish forms now match the same prefix sub-needles. Confirming the looser match works requires authoring `unsafe { std::mem::transmute::<T,U>(...) }` code in body — which would itself fail the invariant. | Inspect `tests/registration_error_surfaces.rs:171` diff — confirm `["std::", "mem::", "transmute"]` (no trailing `(`); existing test must still pass on absence of transmute. |

---

## Validation Sign-Off

- [ ] All items have automated verify OR documented manual-only justification (C1, C2)
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references (2 new sqllogictest files + TEST_LIST registration)
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s for per-task; < 5min for per-wave
- [ ] `nyquist_compliant: true` set in frontmatter after plan-checker pass

**Approval:** pending
