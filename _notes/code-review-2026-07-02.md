# Code Review & Remediation Plan — 2026-07-02

**Scope:** Full codebase review at v0.10.4 (commit `8bc50cf`), covering architecture, code style & structure, correctness, and test coverage/CI.
**Method:** Six focused review passes (architecture; style & structure; parser/text-rewriting correctness; expansion/SQL-generation correctness; FFI/lifecycle/catalog correctness; test coverage & CI), with cross-checking between passes. Findings marked **[verified]** were confirmed empirically — either by executing the code path in a scratch harness, or (for CI findings) via the GitHub Actions API. All other findings were confirmed by tracing the cited code paths in source.
**Prior review:** `_notes/code-review-2026-04-04.md` (v0.5.4). Status of its open items is in §7. The codebase has roughly doubled since (now ~33.2k lines of Rust in `src/`, of which ~14.5k are inline tests, plus the C++ shim).

This document is a remediation **plan** — no fixes have been applied.

---

## 1. Executive summary

The project's engineering discipline is well above average: clean core/shell separation (pure `model`/`expand`/`graph`/`body_parser` vs. thin FFI-gated modules), `clippy::pedantic = deny` with only three global allows, `catch_unwind` at every FFI entrypoint, a type-level connection-borrowing contract backed by AST-walking guard tests, 61 sqllogictest files with hand-computed expected values, per-push fuzzing, and disciplined documentation of accepted debt in TECH-DEBT.md.

Against that backdrop, the review found five clusters of substantive problems:

| # | Cluster | Worst consequence | Where |
|---|---------|-------------------|-------|
| 1 | **Chunk-capacity overflow in bind-materialized table functions** | Heap corruption from ordinary SQL once any `list_*`/`show_*_all`/`describe`/`explain` result exceeds 2048 rows | `cpp/src/shim.cpp` (MS-1) |
| 2 | **CI does not enforce most of the quality gate** | The multi-platform build + sqllogictest workflow has **never run** (0 runs, verified); 10 of 11 Python integration suites and 4 orphaned test files run nowhere in CI | `.github/workflows/` (CI-1..CI-4) |
| 3 | **Silent wrong results in SQL generation** | Fan-trap detector misses metric×metric double-counting; declaration-order-dependent join emission; semi-additive snapshot drops tied rows; nondeterministic derived-metric inlining corruption | `src/expand/` (SG-1..SG-10) |
| 4 | **A family of byte-vs-char UTF-8 bugs** | Panics ("internal error") and mojibake from any non-ASCII input; the v0.10.0 WR-04 fix was applied to one of four copies of the same helper pattern | `body_parser.rs`, `ident.rs`, `util.rs`, `parse.rs` (PA-1..PA-3) |
| 5 | **Round-trip fidelity between the two DDL grammars is convention-guarded** | `get_ddl` output can silently rewire join semantics (dropped `REFERENCES` columns) or fail to re-parse (unquoted names); no property test guards the parse↔render pair | `render_ddl.rs` ↔ `body_parser.rs` (RT-1..RT-3) |

Clusters 1, 2, and the data-loss bug MS-2 are the immediate priorities. Cluster 3 attacks the product's core promise ("the extension writes the GROUP BY and JOIN logic for you") — wrong numbers without an error are worse than a crash for this product category. Clusters 4 and 5 share a single root cause each (duplicated helpers; dual grammar without a machine-checked invariant), so their remediations are structural, not whack-a-mole.

---

## 2. Assessment by dimension

### 2.1 Architecture

**Sound:** The pipeline (parser hook → body parse → model → catalog JSON → expansion → execution via per-call borrowed connections) has a genuinely clean core/shell split. The connection-lifecycle design is exemplary for FFI code: `BorrowedConnection` newtype makes `duckdb_disconnect` untypable, `CatalogReader<'a>` carries a `PhantomData` lifetime, and `tests/no_long_lived_conn.rs` turns the hard-won LIFE-01 lesson into a CI invariant. Transactional DDL (native DML on the caller's connection, no in-memory mirror) is correct and pinned by tests. Kahn toposort/cycle detection in `graph/` is correct; identifier quoting in the expansion layer is escape-correct and idempotent.

**Structural risks:**

- The DDL surface is ~10k lines of hand-rolled string processing maintained as **two independent grammars** (`parse.rs`+`body_parser.rs` for input, `render_ddl.rs` for output) with no systematic round-trip guarantee — and every past bug class in TECH-DEBT (#24, #25, quoted-ident drift, WR-04) traces to exactly this layer.
- The rewrite pipeline **round-trips through a vestigial string form**: `validate_and_rewrite` renders structured data into legacy function-call SQL which `rewrite_to_native_sql` immediately re-parses with a hand-written mini-parser (`parse_table_function_call`, `parse.rs:2421`), unescapes, re-deserializes, enriches, re-serializes. The `FROM YAML FILE` path smuggles fields through an in-band `\x01`-delimited sentinel string (`parse.rs:1385`).
- `parse.rs` is a god module (six responsibilities incl. a layering inversion: `ddl/define.rs:66` calls back into `crate::parse::infer_cardinality`, which is semantic-graph logic, not parsing). The catalog table name `semantic_layer._definitions` appears as a raw literal 53 times across five modules.
- The FFI wire format's column layout is "implicit — agreed out-of-band" (`src/ddl/read_ffi.rs:20-24`) — the same two-place schema coupling TECH-DEBT #12 documented for the retired pipeline, reintroduced by Phase 65 but not recorded.
- The model has no `schema_version`; `Join` carries four generations of field encodings, and legacy rows silently degrade safety checks (see SG-7).

### 2.2 Code style & structure

Production code is smaller than the headline line counts suggest — roughly half of every "oversized" file is inline tests (`sql_gen.rs` is a 569-line module with a 3,500-line test suite attached). unwrap/expect/panic discipline in production code is genuinely good (22 hits, all guarded with SAFETY comments or `unreachable!` rationale). The real debts:

- **~600 lines of copy-pasted FFI scaffolding**: 18 near-identical registration stanzas in `lib.rs:503-887` plus a same-signature `extern "C"` block; 17 dispatchers repeating the same `catch_unwind`/null-check/serialize/publish scaffold; the four `show_*.rs` files are ~90% pairwise-identical (39 differing lines out of ~470 between `show_dims.rs` and `show_metrics.rs`).
- **`body_parser.rs` is the long-function hotspot**: nine `#[allow(clippy::too_many_lines)]`, `parse_keyword_body` at 333 lines, 12 indent levels at the deepest point (`body_parser.rs:1941`).
- **Four coexisting error styles**: exemplary typed `ExpandError`/`QueryError`; `ParseError` with no `Display`/`Error` impl; bare `Result<_, String>` throughout `graph/`, `catalog.rs`, `render_*`; 7 `#[allow(clippy::result_large_err)]` that one `Box` would eliminate.
- **337 comment references to Phase/Plan/Wave/Batch** narrating project history rather than invariants, including tombstones for deleted code.
- Triplicated micro-helpers across the FFI seam (three quoted-string extractors, three error-buffer writers, two buffer publishers with **opposite** ownership semantics — `publish_owned_sql` drops on null out-pointer, `publish_owned_buffer` leaks).

### 2.3 Correctness

The severe findings concentrate in three places:

1. **The C++ emit path** (MS-1): all bind-materialized table functions write their entire row set into a single `DataChunk` whose capacity is 2048; `Vector::SetValue` has no bounds check in release builds.
2. **The expansion layer**: the fan-trap detector checks only metric×dimension pairs and skips window/semi-additive metrics on an incorrect premise; join emission picks the *first* `Join` mentioning an alias rather than the edge that connects it; semi-additive snapshots use `ROW_NUMBER()=1` (drops ties, nondeterministic); derived-metric inlining iterates a `HashMap` and can corrupt previously substituted text on ~half of process runs.
3. **The text layer**: byte-wise scanning through UTF-8 (panics + mojibake); keyword scanners that match inside string literals (a `COMMENT = 'the PRIMARY KEY (id) lives here'` fabricates a PK from comment text); no trailing word-boundary on prefix matching (**`DROP SEMANTIC VIEWS` parses as dropping a view named `s`** [verified]); unquoted view names are case-sensitive, diverging from both DuckDB and Snowflake.

Verified sound: panic containment at FFI boundaries, buffer ownership protocol, RAII on error paths, ROLLBACK of semantic-view DDL, GROUP BY ordinals, quoting in the main expansion path, `escape_sql_arg` correctness, cycle/diamond detection.

### 2.4 Test coverage & CI

The suite is strong where it exists (~14.5k unit-test lines concentrated in the right places, hand-computed sqllogictest values, byte-offset position-invariant proptests, per-push fuzzing). The dominant risks are **operational**:

- The multi-platform build + sqllogictest workflow for feature branches (`BuildQuick.yml`) triggers on `branches: ["!main"]` — a negative-only filter that GitHub Actions never matches. **The workflow has 0 runs in the repo's history** [verified via API]. sqllogictests therefore run in CI only after merge to main (`BuildAll.yml`), if at all.
- 10 of 11 Python integration suites in `just test-all` (concurrency ×3, ADBC ×2, read-only, multi-DB, vtab-crash, caret, large-view) never run in GitHub CI; 4 more Python test files are orphaned entirely (referenced by no runner), including `test_load_extension_twice_idempotent.py`, which `tests/parser_hook_idempotent.rs` names as its behavioral other half.
- Systematic blind spots that map 1:1 to the correctness findings: no test emits >2048 output rows (MS-1); proptest/fuzz identifier generators are uniformly ASCII-lowercase (PA-1/2/3 live exactly there); no render→parse round-trip property (RT-1/2); no differential testing of `semantic_view()` results against hand-written SQL on non-trivial data (SG-* would be caught); semi-additive fixtures contain no NULLs in the ordering dimension.

---

## 3. Findings register

Severities: **C**ritical / **H**igh / **M**edium / **L**ow. Effort: S (<½ day), M (1–3 days), L (week+).

### 3.1 Memory safety & data loss

| ID | Sev | Location | Finding | Effort |
|----|-----|----------|---------|--------|
| MS-1 | C | `cpp/src/shim.cpp:1143-1153` (`sv_list_semantic_views_function`), `:1249-1257` (`sv_emit_varchar_rows`), `:1320-1329` (`sv_emit_varchar_bool_rows`) | Bind-materialized TFs emit all rows into one exec-call `DataChunk` (capacity `STANDARD_VECTOR_SIZE` = 2048), gated only by an `emitted` bool. Row 2049 writes past the vector's data buffer (`Vector::SetValue` is `D_ASSERT`-only in release); `SetCardinality(n>2048)` violates chunk invariants. Reachable via `list_semantic_views()` with >2048 views, `show_semantic_dimensions_all()` with >2048 total dims, large `describe_`/`explain_semantic_view()` output. Affects all 12 read-side TFs using these emitters. | M |
| MS-2 | H | `src/catalog.rs:55-69` | v0.1.0 companion-file migration deletes the sidecar file **even when it could not be read or parsed** (both `if let`s fall through; `remove_file` runs unconditionally) — permanent loss of pre-v0.2 definitions with no error. Conversely, a persistently undeletable file re-imports stale definitions over newer ones (`INSERT OR REPLACE`) on every LOAD. | S |
| MS-3 | M | `src/util.rs:61` | `replace_word_boundary` advances 1 byte after pushing a full char → panics slicing mid-codepoint; reachable at query time via derived-metric/fact inlining and role-playing rewrites over any expression containing a multi-byte char (`'São Paulo'`). Sibling `replace_word_boundary_any` (util.rs:110-115) already has the fix. **[verified]** | S |

### 3.2 CI enforcement gaps

| ID | Sev | Location | Finding | Effort |
|----|-----|----------|---------|--------|
| CI-1 | H | `.github/workflows/BuildQuick.yml:5` | `branches: ["!main"]` — negative-only filters match nothing in GitHub Actions. **Zero runs ever** [verified via Actions API]. Feature branches get no extension build and no sqllogictest run before merge. | S |
| CI-2 | H | `.github/workflows/IntegrationChecks.yml` vs `Justfile:166` | CI runs only `test_ducklake_ci.py` + docs. The other 10 suites in `just test-all` (vtab-crash, caret, ADBC ×2, large-view, multi-DB, read-only, concurrency ×3) are enforced only by developer discipline. | M |
| CI-3 | H | `test/integration/` | Four orphaned test files run nowhere (no Justfile/Makefile/workflow reference): `test_type_inference.py` (458 lines), `test_create_from_yaml_v010.py` (424), `test_drop_on_fresh_readonly_clear_error.py` (167), `test_load_extension_twice_idempotent.py` (114 — the declared behavioral half of `tests/parser_hook_idempotent.rs`). | S |
| CI-4 | L | `.github/workflows/Fuzz.yml` matrix; `Justfile:180` | CI fuzzes 4 of 6 targets — `fuzz_yaml_parse` (user-facing via `FROM YAML`) and `fuzz_parser_override_ffi` never run; neither has seeds; `just fuzz-all` lists only 3 targets. | S |
| CI-5 | L | `test/sql/TEST_LIST` | Currently in sync with the directory (verified), but a new `.test` file not added to TEST_LIST is skipped silently. No CI check enforces sync. | S |
| CI-6 | L | `.github/workflows/CodeQuality.yml:47` | The 80% coverage gate measures only the bundled-feature build (extension-gated FFI code excluded from the denominator) and nextest skips doc tests. Worth a note in MAINTAINER.md so the number isn't misread. | S |

### 3.3 Expansion / SQL generation (silent wrong results)

| ID | Sev | Location | Finding | Effort |
|----|-----|----------|---------|--------|
| SG-1 | C | `src/expand/fan_trap.rs:61-130`; joins from `join_resolver.rs:141-168` | Fan-trap detection loops metric×**dimension** pairs only. Two metrics at different grains (e.g. `SUM(o.amount)` on `orders` + `COUNT(*)` on `line_items`) are never cross-checked, yet the resolver joins both source tables → `SUM` silently inflated by the fan-out. Same hole for classic chasm traps (one metric each on two child tables). Base-table dims are skipped (`fan_trap.rs:88-90`), so even dimensioned queries can hit this. | M–L |
| SG-2 | H | `src/expand/sql_gen.rs:535-537` (replicated at `:211-213`, `:231-233`, `semi_additive.rs:226-229`, `window.rs:187-190`) | Join emission selects the **first** `Join` in declaration order matching the alias on either side, not the edge connecting the alias to already-emitted tables. Depending on relationship declaration order this emits forward-referencing ON clauses (binder error) or drops the connecting join entirely; a child table with FKs to two parents can emit an ON clause referencing a never-joined alias. An existing test fixture (`sql_gen.rs:1680-1699`) exercises the topology but asserts only join order. | M |
| SG-3 | H | `src/expand/facts.rs:362-365` | `inline_derived_metrics` substitutes metric names in nondeterministic `HashMap` order; later passes re-scan earlier replacements, and `.` counts as a word boundary, so a metric named like a column (`revenue` vs `o.revenue`) corrupts the expression into invalid nested-aggregate SQL on a hash-seed-dependent fraction of runs. Sibling `inline_facts` (facts.rs:161-215) already solved this with a single-pass `replace_word_boundary_any`. | S |
| SG-4 | H | `src/expand/semi_additive.rs:117-189, 270-280` | Snapshot semantics use `ROW_NUMBER() = 1` partitioned by queried dims, ordered by NA dims with no tie-break: when fact grain is finer than the queried dims (the normal case), one **arbitrary** row survives per partition instead of all rows at the snapshot value — wrong sums, nondeterministic across runs. Needs `RANK()=1` or `na_dim = MAX(na_dim) OVER (...)`. | M |
| SG-5 | H | `src/expand/semi_additive.rs:364-397` (used at `:103-114`, `:263-291`) | `extract_aggregate_inner/_func` naive first-`(`/last-`)` decomposition of co-queried regular metrics: `SUM(amount) * 0.1` silently **drops** the `* 0.1`; `COUNT(*)` and `COUNT(DISTINCT x)` produce syntax errors; `COALESCE(SUM(x),0)` breaks; derived metrics fall back to a hardcoded `"SUM"`. Any metric co-queried with an active semi-additive metric flows through this. | M |
| SG-6 | H | `src/expand/fan_trap.rs:62-74` | Fan-trap check skipped for window and semi-additive metrics on an incorrect premise: the window CTE's inner aggregate is computed **over the already-fanned join** (inflated before the window function runs), and an effectively-regular semi-additive metric (all NA dims queried) never takes the CTE path yet is still skipped. `test_fan_trap_skips_window_metrics` (window.rs:649-692) pins the wrong behavior. | M |
| SG-7 | M | `src/expand/fan_trap.rs:30-32, 182-184` | `let Ok(graph) = ... else { return Ok(()) }` — the fan-trap **safety check is silently skipped** when graph construction fails (e.g. legacy pre-validation catalog rows), producing mis-aggregated results instead of an error. Ties to the missing `schema_version` (AR-4). | S |
| SG-8 | M | `src/expand/sql_gen.rs:527-553` | All synthesized joins are LEFT JOINs; `COUNT(*)` sourced on a child table counts NULL-extended rows — inflated by one per childless parent. Rewrite to `COUNT(<source_pk>)` or use INNER for metric-only tables. | S–M |
| SG-9 | M | `src/expand/semi_additive.rs:147-176, 202` | Snapshot CTE's ORDER BY uses the NA dim's raw expression, but join resolution is fed only the *queried* dims/metrics — an NA dim on a non-queried, non-base table produces `ORDER BY d.report_date` with no `JOIN dates` → binder error. | S–M |
| SG-10 | M | `src/expand/join_resolver.rs:175-186`; `sql_gen.rs:190-207` | Transitive join walk follows only reverse (root-ward) edges: a needed table two hops *below* the root (`ld → li → o`) never gets its intermediate (`li`) joined → binder error. `expand_facts` adds only the fact's direct source alias with no path walk at all. Such definitions pass define-time validation. | M |
| SG-11 | M | `src/expand/materialization.rs:28-79, 133-165` | Materialization routing matches by name sets only — no schema/consistency check, so a redefined metric or drifted pre-agg table silently changes results; dims-only routed queries emit plain `SELECT` where the normal path emits `SELECT DISTINCT`. | M |
| SG-12 | M | `src/expand/sql_gen.rs:509-521`; `join_resolver.rs:98` | Scoped aliases are round-tripped through strings (`{to_alias}__{rel_name}`) and re-parsed by splitting at the **first** `__` — a user alias containing `__` misparses, silently skipping its join or picking a wrong relationship. Carry `(bare_alias, rel_name)` structs instead. | S–M |
| SG-13 | M | `src/graph/derived_metrics.rs:130-135`; `body_parser.rs:4568`; `resolution.rs:138-161` | Name-uniqueness is only checked when derived metrics exist (and separately for materializations). Duplicate base metrics silently shadow; a dim and metric sharing a name yields duplicate output columns / ambiguous-column errors in CTE paths. Enforce uniqueness across dims/metrics/facts unconditionally at define time. | S |
| SG-14 | L | `src/expand/resolution.rs:112-160`; `sql_gen.rs:30-33` | Qualified-name fallback: `warehouse.region` silently resolves to *any* `region` when no dim on `warehouse` matches; `region` and `o.region` both pass the duplicate check and emit the same column twice. | S |
| SG-15 | L | `src/expand/wildcard.rs:59-71` | `alias.*` expansion matches only `source_table == Some(alias)` — base-table items with `source_table == None` are silently excluded from base-alias wildcards. | S |
| SG-16 | L | `src/expand/fan_trap.rs:35-49` | Cardinality map keyed `(from, to)` collapses role-playing edges — declaration-order-dependent survivor can mask a fan-out or report a wrong relationship name. | S |
| SG-17 | L | `src/expand/sql_gen.rs:65-252` | `expand_facts` never calls `find_using_context` — role-playing ambiguity that raises `AmbiguousPath` on the metrics path silently binds to an arbitrary relationship on the facts path. | S–M |

### 3.4 Parser / text layer

| ID | Sev | Location | Finding | Effort |
|----|-----|----------|---------|--------|
| PA-1 | H | `body_parser.rs:1055, 1012, 1030, 1770`; `parse.rs:443-450, 481, 525-584, 826-841` | Byte-indexed keyword scanning over UTF-8 → **panic** ("byte index N is not a char boundary") on ordinary inputs like `COMMENT = 'café et plus'` or `SHOW SEMANTIC VIEWS aΩΩ`. Surfaces as "internal error (panic)" at the FFI boundary. **[verified]** Fix pattern already exists in-repo (`find_unique`, body_parser.rs:947). | S–M |
| PA-2 | H | `body_parser.rs:1098`; `ident.rs:98` | `bytes[i] as char` Latin-1-izes UTF-8 (`'café'` → `cafÃ©`) in body-parser COMMENT/SYNONYMS extraction and in quoted-identifier parsing (`CREATE SEMANTIC VIEW "café"` stores a mojibake name). The exact bug fixed as WR-04 in `parse.rs` — these are the copies the fix missed. **[verified]** | S |
| PA-3 | H | `body_parser.rs:832-878` | `find_primary_key`/`find_unique` scan with no string-literal awareness: `TABLES (o AS orders COMMENT = 'the PRIMARY KEY (id) lives here')` fabricates `pk_columns=["id"]` from comment text and silently discards the comment. | M |
| PA-4 | M | `parse.rs:101-126` | No trailing word boundary after prefix keywords: `CREATE SEMANTIC VIEWfoo` parses; **`DROP SEMANTIC VIEWS` (plural typo) drops a view named `s`**. **[verified]** | S |
| PA-5 | M | `parse.rs:294-310` | Name-only forms ignore trailing garbage: `DROP SEMANTIC VIEW a b c` / `DESCRIBE ... a CASCADE` execute and discard the tail (the SHOW path already errors — reuse it). | S |
| PA-6 | M | body scanners generally (`split_at_depth0_commas` body_parser.rs:150-187, `find_clause_bounds` :296-319, `extract_paren_content` :970-1001, dot-splits :2170/:2321) | Double-quoted identifiers not tracked: `o."a,b" AS o.x` splits mid-identifier; `o AS "tbl)x"` closes the clause early; `"a.b"` treated as qualifier.dot. Correct helpers (`split_qualified_identifier`, `find_identifier_end`) exist but aren't used here. | M |
| PA-7 | M | body scanners; `parse.rs:645-652, 270, 940` | No SQL-comment handling in the AS-body: comments containing `'`/`(`/`,` corrupt scanning state; trailing comments are silently absorbed into stored expressions (later commenting out generated SQL); `ALTER ... RENAME TO x -- oops` renames to `x -- oops`. | M |
| PA-8 | M | `parse.rs` guards/DML (`catalog.rs:277`, `parse.rs:2184-2205`) vs body-internal matching | Unquoted view names are byte-exact case-sensitive (`CREATE ... Sales` / `DROP ... sales` → "does not exist"), diverging from DuckDB and Snowflake, and inconsistent with the body parser's own `eq_ignore_ascii_case`. Pick one normalization at `normalize_view_name` and apply uniformly. | M |
| PA-9 | M | `body_parser.rs:860-869, 1147-1199` | Table-level COMMENT/SYNONYMS silently dropped when the table has no PK/UNIQUE (remaining text hard-set to `""`); a column literally named `comment` is unusable at depth 0 even quoted. | S–M |
| PA-10 | L | `parse.rs:160-171`; `body_parser.rs:2111`; `parse.rs:645-675`; `read_yaml.rs:23-25`; SHOW token nits (`parse.rs:569, 584`) | Grouped small divergences: block comments don't nest (DuckDB/PG nest per SQL standard); `NON ADDITIVE BY` keyword length hardcoded as 16 (rejects `BY(d)` no-space form); ALTER sub-ops require single spaces; `resolve_bare_name` uses naive `rsplit('.')`; `STARTSWITH`/`LIMIT5` accepted without boundaries. | S each |
| PA-11 | L | `parse.rs:1385, 2000-2013` | YAML-FILE sentinel uses in-band `\x01` field delimiters — a quoted view name containing `\x01` shifts fields. Subsumed by AR-2 (kill the sentinel). | — |

### 3.5 Round-trip fidelity (parse ↔ render)

| ID | Sev | Location | Finding | Effort |
|----|-----|----------|---------|--------|
| RT-1 | H | `render_ddl.rs:72-91`; `parse.rs:1507-1528` | `emit_relationships` never emits `ref_columns`: a relationship declared against a UNIQUE key (`REFERENCES c(alt_key)`) renders as `REFERENCES c`, and re-parsing resolves to the target's **PK** — `get_ddl` output silently rewires join semantics. | S |
| RT-2 | M | `render_ddl.rs:304-305, 51-59` | View name emitted unquoted (names like `"my view"`, `"café"`, `"a.b"`, reserved words produce DDL that re-parses wrong or not at all); `emit_tables` joins `pk_columns` and aliases verbatim with no re-quoting. | S |
| RT-3 | H (structural) | `render_ddl.rs` ↔ `body_parser.rs`; tests | No property-based round-trip: `parse_keyword_body(render_create_ddl(def)) == def` doesn't exist despite `Arbitrary` being derived on every model struct. Grammar drift has happened twice already (TECH-DEBT #24 resolved, #25 open). This is the machine-check that keeps RT-1/RT-2-class bugs from recurring. | M |

### 3.6 FFI / lifecycle / catalog robustness

| ID | Sev | Location | Finding | Effort |
|----|-----|----------|---------|--------|
| FF-1 | M | `parse.rs:2170-2305` (guards + comments at `:2139-2141`, `:2233-2237`, `:2287-2289`) | DROP/ALTER guard statements are separate implicit transactions under autocommit — the "same transaction / snapshot-consistent" comments are wrong outside explicit `BEGIN`. Concurrent DROP: loser silently "succeeds"; concurrent RENAME: loser gets a raw PK error. Document accurately or wrap in an explicit transaction (TECH-DEBT #23 covers only the CREATE case). | S–M |
| FF-2 | M | `cpp/src/shim.cpp:2467, 2530-2531`; `test/sql/v080_transactional_ddl.test` D4 | Read-side runs on fresh connections: inside `BEGIN`, uncommitted **base-table data** is invisible to `semantic_view()` aggregates and an uncommitted `CREATE TABLE` fails the bind probe. Only the catalog-metadata half of this is documented/tested. Document the data-visibility rule; long-term, thread the caller's `ClientContext` (see AR-6). | S (doc) / L (fix) |
| FF-3 | M | `parse.rs:1951-1976` (writes) vs per-call read connections; `lib.rs:508-520` + `catalog.rs:45-70` | ATTACH divergence: after `USE attached_db`, writes resolve `semantic_layer._definitions` in the attached catalog while reads always hit the primary → DDL "succeeds" but the view is invisible (or vice versa). Worse: db-path detection takes the first file from `PRAGMA database_list`, so LOADing after ATTACH from an in-memory primary migrates and **deletes** the attached DB's companion file into the in-memory catalog. No ATTACH coverage in test/sql/. Qualify emitted SQL with the target catalog or pin+document single-catalog support. | M |
| FF-4 | L | `cpp/src/shim.cpp:1373, 1399-1400`; `src/ddl/describe.rs:42-49` and show_* siblings | Wave-2 TFs lack the NULL-argument guard (`describe_semantic_view(NULL)` → "view 'NULL' does not exist") and skip `normalize_view_name`, so quoted-identifier inputs behave differently from `semantic_view()`. | S |
| FF-5 | L | `src/ddl/read_ffi.rs:159-170` vs `parse.rs:1602-1616` | `write_err` truncates mid-UTF-8-codepoint (sibling `write_error_to_buffer` walks back to a boundary) → invalid UTF-8 in `BinderException` text. Consolidate (ST-4). | S |
| FF-6 | L | `read_ffi.rs:184-216`; `ddl/list.rs:149-163`; `table_function.rs:408-423`; `table_function.rs:87-88`; `explain.rs:93-94` | Wire-format length handling: clamp-to-`u32::MAX` / bare `as u32` can desync header from payload; `Vec::with_capacity(count)` trusts an unvalidated u32 count (~100 GB pre-alloc on corruption). Return errors instead of clamping; cap capacity at `len/4`. | S |
| FF-7 | L | `lib.rs:949-967` | `(*access).set_error.unwrap()` executes *after* `catch_unwind` returns in the entrypoint — a null `set_error` aborts the host process instead of failing the load. `if let Some(...)`. | S |
| FF-8 | L | `table_function.rs:614-621` vs `expand/resolution.rs:16-18` | `build_execution_sql` formats `"\"{name}\"::{cast}"` without escaping embedded `"` — the one raw-format exception to otherwise-correct quoting. Reuse `quote_ident`. (Also note: HUGEINT→BIGINT downgrade at `:503-513` is a documented behavior cliff for >int64 SUMs.) | S |
| FF-9 | L | `read_ffi.rs:131-149`; `show_dims.rs:106-107` vs `list.rs:123-131` | Defensive fallbacks mask corruption: any probe failure reads as "no views"; unparseable definitions vanish from `SHOW` but still appear in `list_semantic_views` with empty metadata. Propagate errors distinct from absence; surface unparseable rows. | S |
| FF-10 | L | `catalog.rs:36-40, 254-261` | A SQL-NULL `definition` column reads as "does not exist" to readers while write guards see it as existing — unrecoverable-looking state (manual tampering only). Add `NOT NULL`. | S |
| FF-11 | Info | `table_function.rs:754-763`; `lib.rs:979-992` | Dead layout guards for a transmute deleted in Phase 65 Plan 05 — remove (also releases the stale `duckdb::vtab::Value` import). Resolves prior-review item #7. | S |

### 3.7 Architecture debt

| ID | Sev | Location | Finding | Effort |
|----|-----|----------|---------|--------|
| AR-1 | H | `parse.rs` (5,482 lines; ~2,856 production) | God module: DDL detection, SHOW-clause parsing, validate/rewrite, `infer_cardinality` (semantic-graph logic that `ddl/define.rs:66` calls back into — layering inversion), native-SQL emission, FFI entrypoints, plus 32 of the 53 raw `semantic_layer._definitions` literals. Split into `parse/{detect,show_clauses,rewrite,native_sql,ffi}.rs`; move `infer_cardinality` to `graph/`; move catalog DML emission into a `catalog::writes` module owning the schema name and error wording. | L |
| AR-2 | H | `parse.rs:937, 1287-1289, 1731, 2421-2518, 1344-1386, 1997-2029` | The rewrite pipeline renders structured data into legacy function-call SQL, then immediately re-parses it (`parse_table_function_call`), unescapes, re-deserializes, enriches, re-serializes; the YAML-FILE path smuggles fields through a `\x01` sentinel. Replace with a structured `RewriteAction` enum (`Create { kind, name, def, comment } | YamlFile {…} | ReadSide(String)`); derive both output forms from it; delete the mini-parser and sentinel. Eliminates PA-11 and a whole class of escaping invariants. | L |
| AR-3 | M | `src/ddl/read_ffi.rs:20-24` + C++ binds | FFI wire format's column layout is implicit/out-of-band — the TECH-DEBT #12 coupling reintroduced by Phase 65 without a debt entry. Make the format self-describing (col count + type tags asserted on the C++ side) or generate both sides from one descriptor; at minimum add the TECH-DEBT entry. | M |
| AR-4 | M | `model.rs:316-355, 381-392`; `render_ddl.rs:294-299` | No `schema_version` in stored JSON; `Join` carries four field-generations; parallel type-inference vecs are dead for new rows. Legacy rows silently degrade safety (SG-7) or hard-error rendering. Add `schema_version` + one-time upgrade pass in `init_catalog` (precedent: the v0.1.0 migration), then delete dead fields. | M |
| AR-5 | M | `parse.rs:2651-2682, 2733-2779` | Validate-twice error architecture (override defers with rc=2; parse_function re-runs the full rewrite) relies on an **unenforced** purity/idempotence invariant — the code documents that the two runs can diverge. State the invariant as a hard rule in `rewrite_to_native_sql`'s docs; optionally cache `(query-hash → ParseError)` so parse_function replays the identical message. | S |
| AR-6 | M | TECH-DEBT #19 vs `cpp/src/shim.cpp` post-Phase-65 | TECH-DEBT #19's stated blocker ("BindInfo doesn't expose the connection") is stale: C++ binds now *have* the caller's `ClientContext&` and choose to open fresh connections. Re-evaluate running catalog reads on the caller's context (would fix the read-side transaction asymmetry, FF-2) or document why DuckDB forbids it; update the entry either way. | M (investigation) |
| AR-7 | L | `parse.rs:53, 2542-2573` + shim `rust_state` | Dead abstraction: empty `OverrideContext` + make/drop functions survive "for FFI shape compatibility". Retire. | S |
| AR-8 | L | `lib.rs:901`; TECH-DEBT header/footer; `Justfile:118-120` | Drifted constants/docs: `MINIMUM_DUCKDB_VERSION = "v1.4.4"` vs pin v1.5.4; TECH-DEBT footer says "Milestone: v0.8.0"; Justfile still documents `SKIP_UNTIL_PLAN_02` for a landed migration. Add a unit test asserting `.duckdb-version` ↔ Cargo pin ↔ `MINIMUM_DUCKDB_VERSION` consistency. | S |
| AR-9 | L | `build.rs:408-417, 444-453` | Windows amalgamation patches soft-fail with `cargo:warning` when markers move — a DuckDB bump can skip a patch and surface later as opaque SDK-specific compile errors. Make patch-miss a hard error for known-affected versions. | S |

### 3.8 Test-coverage gaps

| ID | Sev | Finding | Effort |
|----|-----|---------|--------|
| TC-1 | H | No test emits >2048 output rows from any TF (`test_cv4_large_result` aggregates 10k inputs down to 100 output rows; all sqllogictest outputs <20 rows). This is the regression test for MS-1. Add a dims-only query over `generate_series(1, 5000)` asserting count and first/last values. | S |
| TC-2 | H | No differential testing: expansion correctness rests on SQL-string shape assertions + hand-computed values on 2–5-row fixtures. Build a Python harness seeding a star schema with a few thousand randomized rows (NULL FKs/measures included), comparing every dims×metrics combination between `semantic_view()` and hand-written SQL. Would have caught most of SG-1..SG-10. | M–L |
| TC-3 | H | Proptest/fuzz identifier generators are uniformly `[a-z_][a-z0-9_]*` (parse_proptest.rs:34, yaml_proptest.rs:14, ident proptest `[\x20-\x7E]`) — they systematically avoid the quoted/unicode/delimiter-bearing shapes behind PA-1/2/3/6 and the phase-64/67/68 regressions. Add quoted/unicode/keyword arms; add one end-to-end sqllogictest with `"wéird name"` through CREATE→query→DESCRIBE→GET_DDL. | S–M |
| TC-4 | M | `expand_proptest.rs` is weaker than it looks: fixed 2-fixture universe (never generates facts/window/semi-additive/derived); `group_by_section.contains("1")` matches any digit; one property takes `Just(...)` (a unit test in proptest costume); all string-containment, SQL never executed. Drive `expand()` with `arb_definition()`-generated defs; parse GROUP BY exactly; bind expanded SQL `LIMIT 0` against in-memory DuckDB. | M |
| TC-5 | M | `output_proptest.rs` validates `test_helpers::read_typed_value`, a mirror of a production function that **no longer exists** (retired in Phase 65; production is zero-copy vector references + C++ streaming). The stale claim is at `lib.rs:77`. Fix the comment; ensure CV-1-style typed-output assertions run in CI. | S |
| TC-6 | M | Restart-persistence claim is stale: Makefile (~:96) and `_excluded` test headers say it's "verified via cargo test Rust integration tests" — no such test exists (only rollback is tested, catalog.rs:504). Real coverage lives in `test_readonly_load.py`, which doesn't run in CI (CI-2). Add the Rust open→persist→drop→reopen→lookup test; fix the comments. | S |
| TC-7 | M | Semi-additive/window tests never put NULLs in the ordering dimension — `NULLS FIRST/LAST` is pinned as syntax only. Add NULL `report_date` rows and assert which balance wins under `DESC NULLS FIRST` vs `LAST`. (Also the data-level regression test for SG-4.) | S |
| TC-8 | M | Pure-logic helpers inside untested FFI modules: `build_execution_sql` + `type_id_to_cast_sql` (`table_function.rs:560-627` — the cast map guarding the vector-reference type contract), name-list wire decoding, `explain.rs` formatting, `ddl/list.rs`/show_* SQL builders — zero direct tests. Add `#[cfg(test)]` coverage incl. malformed/truncated buffers. | M |
| TC-9 | L | Fuzz oracles are thin (`fuzz_sql_expand` asserts non-empty + `WITH` prefix). Add balanced-quote/paren checks or a LIMIT-0 bind; add non-ASCII and annotation-bearing seeds; add a direct `parse_keyword_body` target and a render→parse round-trip target (complements RT-3). | S–M |
| TC-10 | L | `test/infra/test_phase34_infra.sh` referenced nowhere — wire into CI or delete. The per-file process isolation workaround (Makefile:113-116) pins its motivating DuckDB crash nowhere — add an expected-fail probe so the workaround can be retired when upstream fixes. | S |

### 3.9 Style & structure

| ID | Sev | Finding | Effort |
|----|-----|---------|--------|
| ST-1 | M | `lib.rs:330-478, 503-887`: 18 same-signature extern decls + 18 copy-pasted registration stanzas (~300 lines). Table-driven registration (`const REGISTRATIONS: &[(&str, RegisterFn)]` + loop + small macro for the extern block) collapses this to ~40 lines and makes TF #19 a one-line diff. | M |
| ST-2 | M | `ddl/show_{dims,metrics,facts,materializations}.rs` are ~90% identical (39/470 differing lines between dims and metrics); 17 dispatchers share the same catch_unwind/null-check/serialize/publish scaffold (~600 duplicated lines). Add `run_dispatcher(...)` in `read_ffi.rs` + one generic `collect_entities` over a shared `EntityRow`; the four files become one ~150-line module. | M |
| ST-3 | M | `body_parser.rs` decomposition: convert to a directory module (`body_parser/{clause_bounds,tables,relationships,metrics,window,annotations}.rs` — the file already groups this way); extract the ORDER-BY modifier loop (`:1911-1961`) to kill the 12-level nesting and its duplicated error arm. Do together with PA-1/PA-6 fixes to avoid double-churn. | M–L |
| ST-4 | M | Consolidate the triplicated FFI seam helpers: three quoted-string extractors (`parse.rs:353, 1300`; `body_parser.rs:1078`), three error-buffer writers (`parse.rs:1602`; `read_ffi.rs:159`; `alter_helpers_ffi.rs`), two buffer publishers with opposite null-out-pointer semantics (`parse.rs:1682` drops — the safer contract — vs `read_ffi.rs:229` leaks). This is also the *structural* fix for the PA-2/FF-5 "fix landed in one copy" pattern. | M |
| ST-5 | M | Error handling: give `ParseError` `Display` + `Error` impls (callers currently reach into `.message`); `Box` the large `ExpandError` payloads to delete all 7 `result_large_err` allows; newtype or alias the bare `String` errors in `graph/`/`catalog.rs`. | S–M |
| ST-6 | M | Magic strings: `semantic_layer._definitions` ×53 and "does not exist" wording ×31 (independently formatted in 10+ files, pinned by sqllogictests). One `DEFINITIONS_TABLE` const + one `view_not_found_msg()` helper. Prerequisite-ish for AR-1's catalog-writes module. | S |
| ST-7 | L | 337 Phase/Plan/Wave/Batch comment references; worst are tombstones for deleted code (`show_dims.rs:98-102, 228-238`; `lib.rs:307-324`; `parse.rs:17-45`). Delete tombstones; rewrite keeper-comments invariant-first; move chronology to CHANGELOG. | M (mechanical) |
| ST-8 | L | Grouped nits: `expand/test_helpers.rs` spells out every struct field vs graph's parametric builders (promote one builder style + `..Default::default()`); `FactName` newtype missing (`expand/types.rs:144-148`); `format_json_array` lives in `describe.rs:141` (move to util); two unrelated `facts.rs` modules; `#[allow(dead_code)]` on live-under-extension modules should be `#[cfg_attr(not(feature = "extension"), allow(dead_code))]` (`expand/mod.rs:10`, `materialization.rs:88`); module docs absent from 24/40 files and `parse.rs`'s good header uses `//` not `//!`; ~190 lines of drift-prone mirror code in `lib.rs` `test_helpers` under a blanket pedantic allow (factor shared readers into one always-compiled module). | S each |

---

## 4. Remediation plan

Phased so that each phase is independently shippable and `just test-all` stays green throughout. Within a phase, items are ordered by dependency.

### Phase R0 — Stop the bleeding (memory safety, data loss, CI enforcement)

*Goal: no known memory-unsafety reachable from SQL; CI actually enforces the quality gate. Est. ~1 week.*

1. **MS-1 + TC-1**: Rework the three shim emitters to chunked emission — store a row cursor in the local state, emit `min(remaining, output.GetCapacity())` rows per exec call. Land the >2048-row sqllogictest/Python regression **first** (it should fail before the fix on an ASAN/debug build).
2. **CI-1**: Fix `BuildQuick.yml` trigger (`branches-ignore: [main]` or a positive glob + `!main`). Verify via a test push that the distribution workflow (build + sqllogictest) runs on a feature branch.
3. **CI-2 + CI-3**: Add an `integration` job to IntegrationChecks.yml running the `just test-all` Python suites (or a `just test-ci` alias). Triage the four orphaned test files: wire in or delete with a note; `test_load_extension_twice_idempotent.py` must be wired (it's the behavioral half of a structural test's claim).
4. **MS-2**: Companion-file migration: delete the file only after successful parse+import; surface read/parse failures as load-time errors.
5. **MS-3**: Fix `replace_word_boundary` to advance by `ch.len_utf8()` (mirror `replace_word_boundary_any`); add a non-ASCII unit test.
6. **CI-4, CI-5**: Add the two missing fuzz targets to the CI matrix + seeds dirs; add a TEST_LIST↔glob sync check to CI (a 5-line script step).
7. **FF-7**: Null-guard `set_error` in the entrypoint.

### Phase R1 — Correct results (expansion/SQL-gen)

*Goal: no known silent-wrong-number path; anything the generator can't handle correctly is a define-time or query-time error. Est. ~2–3 weeks. Land TC-2 (differential harness) first — it is both the acceptance test and the regression net for everything else in this phase.*

1. **TC-2**: Differential harness (Python): randomized star schema (thousands of rows, NULL FKs/measures, childless parents, multi-child fan-outs), every dims×metrics combination checked against hand-written SQL. Run it in CI (seeded, bounded runtime).
2. **SG-3**: Single-pass derived-metric inlining via `replace_word_boundary_any`-style combined substitution (mirrors the existing `inline_facts` solution). Small, isolated, do early.
3. **SG-2 + SG-12 + SG-10**: Join-emission overhaul — carry chosen edges/`(bare_alias, rel_name)` structs from the resolver instead of re-searching `def.joins` by name and re-parsing `__`-joined strings; extend the transitive walk to forward (FK-side) chains; give `expand_facts` the same path resolution. These three share the same code, fix together.
4. **SG-1 + SG-6 + SG-7 + SG-16**: Fan-trap detector: add metric×metric grain checks; run the check on window metrics' inner aggregation and on semi-additive metrics not taking the CTE path; make an unbuildable graph an error, not a skipped check; key the cardinality map by relationship, not `(from,to)`. Decide policy: erroring on multi-grain queries is acceptable for now; per-grain CTE aggregation (the real fix) can be a follow-up milestone.
5. **SG-4 + SG-5 + SG-9 + TC-7**: Semi-additive rework — `RANK()=1` or `MAX() OVER` snapshot semantics with defined tie behavior; validate co-queried metric expressions at define/query time and error on shapes the decomposer can't handle (`COUNT(*)`, `DISTINCT`, arithmetic-wrapped, derived) rather than silently mangling; join the NA dim's source table (with intermediaries) into the snapshot CTE; add NULL-bearing and tie-bearing fixtures.
6. **SG-8**: `COUNT(*)` on non-base sources → rewrite to `COUNT(source_pk)` (or document + error). Verify via the differential harness.
7. **SG-13 + SG-14 + SG-15 + SG-17**: Define-time name-uniqueness across dims/metrics/facts; remove the wrong-table qualified-name fallback (error instead); include unqualified base items in base-alias wildcards; run role-playing ambiguity detection on the facts path.
8. **SG-11**: Materialization routing: schema/column-existence validation at routing time, `SELECT DISTINCT` for dims-only routed queries, and a documented staleness stance (at minimum a docs page; ideally invalidate on `CREATE OR REPLACE`).

### Phase R2 — Text-layer correctness + round-trip guarantee

*Goal: non-ASCII and quoted identifiers work end-to-end; `get_ddl` output provably re-parses to the same definition. Est. ~2 weeks.*

1. **ST-4** first (consolidate the four copies of quoted-string extraction, the error-buffer writers, and the buffer publishers into one `ffi_util`/`util` implementation each, using the parse.rs "both-or-drop" publish contract and the boundary-aware error writer) — then **PA-1 + PA-2** are fixed once, in one place, instead of four (also resolves FF-5).
2. **PA-4 + PA-5**: Trailing word-boundary after prefix keywords; reject trailing garbage in name-only forms. (Small; prevents the `DROP SEMANTIC VIEWS` foot-gun.)
3. **PA-8**: Decide and implement one case-normalization rule for view names (recommend: fold unquoted to lowercase, preserve quoted — Snowflake-consistent per project convention), applied at `normalize_view_name` and used by guards, DML, and lookups. Migration note for existing catalogs with mixed-case names.
4. **PA-3 + PA-6 + PA-7 + PA-9**: Make body scanners string-, quote-, and comment-aware. This is the natural moment for **ST-3** (split `body_parser.rs` into a directory module) — restructure and fix together, against the strengthened test net from item 6.
5. **RT-1 + RT-2**: Emit `ref_columns` when they differ from the target PK; quote view names/aliases/pk columns in `render_ddl` via the shared quoting helpers.
6. **RT-3 + TC-3 + TC-9**: Add the `Arbitrary`-driven `parse(render(def)) == def` proptest and a fuzz round-trip target; extend identifier strategies with quoted/unicode/keyword arms across parse/yaml/ident proptests; add the end-to-end `"wéird name"` sqllogictest. Expect these to fail until items 1–5 land — add first, fix until green.
7. **PA-10** batch: boundary/whitespace nits, nested block comments, `resolve_bare_name`, `NON ADDITIVE BY` offset.

### Phase R3 — Architecture hardening

*Goal: convert convention-guarded invariants to machine-guarded ones; shrink the change surface. Est. ~3–4 weeks, incremental.*

1. **AR-2**: Introduce `RewriteAction` and delete the string ping-pong (`parse_table_function_call`, arg escaping round-trips, the `\x01` sentinel → PA-11 disappears). Highest-leverage single refactor in the codebase.
2. **AR-1 + ST-6**: Split `parse.rs`; move `infer_cardinality` to `graph/`; create `catalog::writes` owning `DEFINITIONS_TABLE`, guard SQL, and `view_not_found_msg()`. Do after AR-2 (the split falls out more cleanly once the rewrite pipeline is structured).
3. **AR-4 + SG-7 follow-through**: Add `schema_version` to stored JSON + upgrade pass in `init_catalog`; delete dead parallel-vec fields and pre-Phase-24 `Join` encodings after the upgrade pass handles them; make legacy/unparseable rows a clear error everywhere (with FF-9's diagnostics).
4. **ST-1 + ST-2**: Table-driven registration; shared dispatcher scaffold + generic entity collection for show_*. (~700 lines removed; do before adding any new TF.)
5. **AR-3**: Self-describing wire format (count + type tags, asserted C++-side) or single-descriptor generation; add the TECH-DEBT entry regardless.
6. **FF-1 + FF-2 + FF-3**: Fix the inaccurate transactionality comments and wrap DROP/ALTER guard+DML in an explicit transaction when not already in one; document read-side data-visibility under open transactions; pick and implement an ATTACH stance (qualify emitted SQL with the target catalog, or explicitly pin single-catalog support with a guard + docs + a regression test). **AR-6**: time-boxed investigation of running reads on the caller's `ClientContext` — if it works, it subsumes FF-2 and retires TECH-DEBT #19.
7. **FF-4, FF-6, FF-8, FF-9, FF-10, FF-11, AR-5, AR-7, AR-8, AR-9**: batch of small hardening items (NULL guards + name normalization for wave-2 TFs, wire-length error handling, `quote_ident` in `build_execution_sql`, probe-error propagation, `NOT NULL` on `definition`, dead-guard removal, idempotence invariant docs, `OverrideContext` retirement, version-consistency test, hard-fail Windows patches).

### Phase R4 — Test depth + hygiene

*Goal: coverage that would have caught this review's findings; a codebase that reads as invariants, not chronology. Ongoing/opportunistic.*

1. **TC-4**: Strengthen `expand_proptest` (generated definitions, exact GROUP BY parsing, LIMIT-0 bind oracle).
2. **TC-5 + TC-6**: Fix stale mirror-comment and restart-persistence claims; add the file-backed reopen test; ensure typed-output CV-1 assertions run in CI.
3. **TC-8**: Unit tests for the cast table, wire decoding, explain formatting, show_* SQL builders.
4. **TC-10**: Wire or delete `test_phase34_infra.sh`; add the expected-fail probe for the per-file process isolation workaround.
5. **ST-5**: Error-type cleanup (`ParseError: Display+Error`, boxed `ExpandError`, graph error newtype).
6. **ST-7 + ST-8**: Phase-comment sweep (tombstones out, invariants stay); fixture-builder unification; naming/module-doc/`#[cfg_attr]` nits; CI-6 note in MAINTAINER.md.

### Sequencing rationale

- R0 before everything: MS-1 is the only memory-safety defect reachable from ordinary SQL, and until CI-1/CI-2 land, *every other fix in this plan is unverified by CI on its own PR*.
- R1 before R2/R3: wrong numbers are this product's worst failure mode, and the differential harness (TC-2) built there also protects the later refactors.
- ST-4 (helper consolidation) is deliberately scheduled *before* the UTF-8 fixes it hosts, and RT-3/TC-3 (the round-trip proptest + generators) *before* the final text-layer fixes — in both cases the structural/test change is what prevents recurrence, not the point fix.
- R3's big refactors (AR-1/AR-2) come after the correctness phases so they're performed against a trustworthy test net, and after ST-1/ST-2 shrink the surface they must move.

---

## 5. Cross-cutting themes

1. **Fixes that didn't propagate to copies.** WR-04 (UTF-8 extraction) fixed one of four copies; `replace_word_boundary_any` fixed the byte-advance bug but not its sibling; `inline_facts` solved single-pass substitution but `inline_derived_metrics` didn't; the boundary-aware error writer exists next to a truncating one; `find_unique` scans bytes correctly next to scanners that don't. The remediation is consolidation (ST-4, util unification), not more point fixes — and a review habit: when fixing a helper, grep for its siblings.
2. **Convention-guarded invariants.** The dual DDL grammar, the rewrite idempotence requirement, the FFI wire format, TEST_LIST membership, and the `.duckdb-version`↔pin↔`MINIMUM_DUCKDB_VERSION` triple are all correctness-critical and all enforced only by convention. The project already knows the fix — it machine-guarded the connection-lifecycle invariant with newtypes + AST tests. RT-3, AR-3, AR-5, CI-5, AR-8 apply the same philosophy elsewhere.
3. **Silent degradation over loud failure.** Skipped fan-trap checks on unbuildable graphs, probe failures reading as "no views", unparseable definitions vanishing from SHOW, trailing garbage ignored, guard races "succeeding", comments/metadata silently dropped, wrong-table name fallback. For a semantic layer, the correct bias is loud: if the engine can't prove the query safe, it should error, not guess.
4. **The tests avoid exactly the hostile inputs users provide.** ASCII-only generators, <20-row outputs, no NULLs in ordering dims, tiny fixtures, string-shape assertions. TC-1/2/3/7 are the highest-leverage test investments and each doubles as the acceptance criterion for a correctness fix.

---

## 6. Status of the 2026-04-04 review's items

| Prior item | Status |
|---|---|
| 1. `catalog_insert` TOCTOU | Superseded — the in-memory-mirror catalog was replaced in v0.8.0 by native DML. The race re-appears in new form as the DROP/ALTER guard windows (FF-1); TECH-DEBT #23 covers only the CREATE case. |
| 2. String-interpolated persistence SQL | Still present in evolved form: the parser override emits native DML with manual escaping (`escape_sql_arg`). Traced correct in this review, including nested-quote cases — but AR-2 (structured `RewriteAction`) is the durable fix. |
| 3. Parallel-Vec `column_type_names`/`column_types_inferred` | Open, and now effectively dead fields kept for v0.7.1-era rows — folded into AR-4 (schema_version + field cleanup). |
| 4. Test-fixture duplication in `sql_gen.rs` | Open (ST-8). |
| 5. File-backed catalog round-trip test | Open, and the Makefile comment claiming it exists is stale (TC-6). |
| 6. `build.rs` size/structure | Open; only the sharper hazard (soft-fail Windows patches, AR-9) is re-flagged here. |
| 7. `transmute` layout dependency in `table_function.rs` | **Resolved** — the transmute was deleted in Phase 65; only its dead layout guards remain (FF-11). |

---

## 7. Suggested TECH-DEBT.md updates

- Add an entry for the FFI wire-format schema coupling (AR-3) — the reintroduction of #12's pattern.
- Update #19 (read-side transaction visibility): the stated blocker is stale post-Phase-65 (AR-6); note the base-table-data visibility consequence (FF-2), not just catalog metadata.
- Extend #23 (DDL race guards) to cover the DROP/ALTER guard windows and the inaccurate "same transaction" comments (FF-1).
- Note under #25 (quoted idents in NAB/OVER) that the body-scanner quote-awareness work (PA-6) is the umbrella fix.
- Refresh the header/footer (still says milestone v0.8.0, pin "=1.4.4").
