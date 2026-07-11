# Code Review — 2026-07-11

**Scope:** Full codebase review at v0.10.4 (commit `cdd9413`, ~38.4k lines of Rust in `src/` of which roughly half is inline tests, plus the C++ shim). Focus per request: (a) does the code carry cruft of accreted reimplementations, or is it how we'd write it from scratch with hindsight; (b) can test coverage of parsers and correctness of output be improved; (c) is the Rust idiomatic.
**Method:** Five focused review passes (parser layer; expansion/SQL generation; Rust idiomaticity; test coverage; catalog/DDL/FFI/C++), each instructed to read TECH-DEBT.md first and not re-report catalogued debt. Findings marked **[verified]** were confirmed empirically — executed against the pinned DuckDB 1.4.4 in a scratch harness, or traced through both state machines with a concrete input. Everything else was confirmed by reading the cited code; residual-uncertainty items are marked SPECULATIVE.
**Prior review:** `_notes/code-review-2026-07-02.md` (also v0.10.4, pre-R3/R4 remediation). This review deliberately checked what those rounds fixed, what they missed, and what they left half-done.

This document is a review, not a remediation plan — no fixes have been applied.

---

## 1. Executive summary

The R3/R4 remediation rounds did real work. The big historical migrations are genuinely excised (no `_base` CTE remnants, legacy join resolution deleted, the `:=` function-call era gone from the front door), FFI memory discipline is exemplary (RAII buffer ownership, `catch_unwind` at all 20+ `extern "C"` boundaries, the `BorrowedConnection` newtype + AST-walking guard test, 21/21 FFI symbols cross-check with no orphans, zero Rust-side statics), and quoting in the expansion layer is centralized, escape-correct, and idempotent. On idiomaticity the codebase grades **A–**: a strong Rust team would call this well above the median for FFI-heavy extension code.

Against that backdrop, this review found **one confirmed silent-wrong-results bug**, **two systemic diseases** that directly answer the "accretion" question, and a **well-defined gap in output-correctness testing** that would have caught the bug.

| # | Cluster | Worst consequence | Where |
|---|---------|-------------------|-------|
| 1 | **Semi-additive snapshot partitions by dimension *alias*, which DuckDB binds to a same-named physical column** | Silent wrong numbers for any semi-additive query where a dimension's expression differs from its bare column (`upper(o.region) AS region`) — [verified] on DuckDB 1.4.4 | `src/expand/semi_additive.rs:187-221` (E-1) |
| 2 | **The parser layer has no lexer/token stream** — ~15 hand-rolled keyword matchers, ~8 quote-state loops, two divergent comment grammars, two divergent dollar-quote grammars | Every previously-remediated parser bug class (PA-2 mojibake, PA-5/9 silent discard, PA-10 exactly-one-space, TECH-DEBT #24/#25 whitespace tokenizer) has **fresh, un-catalogued instances** in this snapshot; worst is silent data loss of a misplaced `COMMENT` in a TABLES entry | `src/body_parser/`, `src/parse/` (P-1..P-15) |
| 3 | **Half-finished consolidations**: the abstractions R3 built (`run_dispatcher`, generic C++ varchar helpers, `normalize_view_name`, `references_name`, canonical wording, `render_ddl` emitters) each stopped partway through rollout | Every behavioural divergence found on the read side lives in a **non-migrated copy**: `get_ddl` misses name normalization, DESCRIBE silently drops window `frame_clause`, `list_semantic_views` skips the trailing-bytes wire check, `show_columns` breaks canonical wording | `src/ddl/`, `cpp/src/shim.cpp` (C-1..C-7) |
| 4 | **Case-sensitivity contract split between validation and inlining** | `profit AS REVENUE - Cost` passes CREATE validation but is not inlined; the same query then errors or silently works depending on which *other* metrics are co-queried — [verified] | `src/expand/facts.rs`, `src/util.rs`, `src/graph/` (E-2, E-3, E-5) |
| 5 | **The differential test oracle excludes exactly the features most likely to be wrong** | Semi-additive, window, wildcard, and role-playing outputs are pinned only by 2–5-row hand-picked fixtures; the oracle that would have caught E-1 stops at "supported core" | `test/integration/test_differential.py` (T-1) |

**Direct answers to the three questions asked:**

**(a) Accretion.** The headline file sizes mislead: `sql_gen.rs` is ~510 lines of production code + ~4,180 of phase-named tests; `parse/rewrite.rs` is 658 + ~2,990; `body_parser/mod.rs` is 401 + ~2,394. The production code is *not* a pile of layered reimplementations of the same feature — it is one dispatcher over four sibling emission strategies, cleanly routed. What it **is**, in the parser layer, is *hardened accretion*: a well-tested pile of point solutions to problems a token stream would not have. The evidence is trajectory, not aesthetics — four separately-remediated bug classes all re-emerged in new grammar slots because each new slot re-hands the author a raw `&str`. Elsewhere the accretion is subtler: six copies of the word-boundary reference scanner (two with drifted boundary predicates), four near-identical SELECT/FROM/JOIN/GROUP BY emitters (one of which forgot the alias-shadowing defense the standard path has — that's bug E-1), three independent graph traversals over the same join edges, and 13 `fk_columns.is_empty()` legacy guards that are mostly unreachable since SG-7 hard-errors. Plus a visible sediment of stale comments describing retired architectures (C-10) and confirmed-dead code (C-8, P-10).

**(b) Tests.** Parser coverage is strong — the feared classics (escaped quotes, unicode carets, keywords in string literals, leading comments, trailing commas) are all covered — with narrow, cheap holes: the duplicate-clause error path has zero tests, empty clause bodies are unpinned, in-body comments work only by construction, and the hostile-identifier proptest generators never reach `plan_rewrite`'s CREATE route. Output correctness is genuinely layered (string asserts → bind oracle → sqllogictest values → differential harness), **but** the differential oracle's own SCOPE comment excludes semi-additive/window/wildcard/role-playing — precisely where tie-breaking, NULL ordering, and alias binding produce wrong numbers small fixtures can't catch. The feature-interaction matrix is mostly empty (semi-additive × anything: one cell).

**(c) Rust.** Idiomatic, with the unsafe/FFI seam the strongest part (grade A–). Non-test `unwrap` count ≈ 10 in 38k lines, all guarded. The gaps: a stringly-typed error core (`Result<_, String>` in ~30 signatures, positions manufactured after the fact, a 5×-pasted `map_err` ladder), SQL-escaped-vs-raw strings distinguished only by variable naming in the injection-adjacent emission path, 6- and 9-field positional tuples with "index 7" comments compensating for missing structs, and a 9-closure `resolve_names` signature that has **already** had an error constructor transposed (dead today, booby trap tomorrow).

**Is a rewrite warranted?** No — for the expansion layer, targeted incremental refactoring under the existing executed-SQL tests. For the parser layer, **yes to an incremental lexer+cursor migration** (~1,400 new lines replacing ~2,900 of scanning code): the ~5,400 lines of regression tests are an excellent immune system for a disease the architecture keeps re-contracting, and they make the migration low-risk. Port TABLES first (worst findings), one clause per phase.

---

## 2. Confirmed bugs (fix first)

### E-1 — HIGH [verified]: semi-additive `RANK()` partitions/orders by the dimension alias; DuckDB binds it to a same-named physical column

`src/expand/semi_additive.rs:187-196` builds `PARTITION BY` from `quote_ident(&d.name)` (the alias), and `:203-221` uses the alias in `ORDER BY` when the NA dim is queried. The `RANK() OVER (...)` is emitted **inside the snapshot CTE's own SELECT**, where all base/joined physical columns are in scope — and DuckDB resolves an identifier in a window clause to a FROM-clause column before a lateral select alias.

Repro (DuckDB 1.4.4 and 1.5.4): table `o(region, amount, snap_date)` with rows `('us',1,'2024-01-01')`, `('US',2,'2024-01-02')`; dimension `region AS upper(o.region)`; semi-additive `SUM(amount)` `NON ADDITIVE BY (snap_date DESC)`. Generated CTE: `... upper(o.region) AS "region", RANK() OVER (PARTITION BY "region" ORDER BY o.snap_date DESC ...)`. `PARTITION BY "region"` binds to raw `o.region` → partitions `{us}`,`{US}` → both rows rank 1 → result `('US', 3)` instead of `('US', 2)`. No error.

The bitter detail: the standard path knows this hazard — it uses `GROUP BY 1, 2, ...` ordinals precisely "to avoid ambiguity when an expression matches its alias" (`sql_gen.rs:497-499`). The semi-additive emitter is a later copy that didn't inherit the defense, and its tests only use dimensions whose expr *is* the bare column, so alias and column coincide.

**Fix:** never reference sibling select aliases from window clauses in the same SELECT — repeat the dimension expression in PARTITION BY/ORDER BY, or two-level CTE (inner projection, then rank over projected columns — `window.rs` gets this right by construction since its OVER lives in the outer query over `__sv_agg`).

### E-2 — MED-HIGH [verified]: validation resolves references case-insensitively; textual inlining substitutes case-sensitively

`inline_derived_metrics` (`src/expand/facts.rs:506-514`) builds needles from **lowercased** metric names but byte-compares against the **raw-case** expression via `replace_word_boundary_pairs` (`src/util.rs:111-145`); `inline_facts` is case-sensitive on as-declared names. The validators (`graph/derived_metrics.rs:189-192`, `graph/facts.rs:23-53`) lowercase both sides. Result: `profit AS REVENUE - Cost` over metrics `revenue`/`cost` passes CREATE and toposort but is not inlined; the raw identifiers leak into generated SQL, and observed behaviour **depends on co-queried metrics** (verified on 1.4.4): alone → binder error; co-queried with `revenue` and `cost` → DuckDB's lateral alias resolution silently rescues it; name shadows a physical column → GROUP BY error.

**Fix:** make the substitution scanner case-insensitive using the dual-buffer trick `find_fact_references` already uses (lowercased haystack for matching, splice raw string by byte offsets).

### P-1 — HIGH [verified by trace]: TABLES entry silently discards text between the source-table name and PRIMARY KEY / UNIQUE

`src/body_parser/tables.rs:135-167`: after capturing the table name, the parser scans the *entire* post-name slice for `PRIMARY KEY` and slices from wherever it finds it — `_pk_start` is deliberately ignored, so everything between the name and `PRIMARY` is never validated. The UNIQUE loop (`:169-199`) has the identical hole.

```sql
CREATE SEMANTIC VIEW v AS
  TABLES (o AS orders COMMENT = 'load-bearing doc' PRIMARY KEY (id))
  DIMENSIONS (o.d AS o.id);
```
parses **successfully** with `comment: None` — a naturally-misplaced COMMENT is silently destroyed. This is the PA-5/PA-9 silent-discard class; the PA-9 fix covered *trailing* text but missed the *pre-constraint* gap. **Fix:** after `name_end`, require the next token to be `PRIMARY`/`UNIQUE`/`COMMENT`/`WITH`/end-of-entry (or consume sequentially — §6).

### P-2 — MED [verified by trace]: trailing garbage after annotations silently accepted; duplicate COMMENT silently dropped

`src/body_parser/annotations.rs:133-170` independently searches the annotation region for one `COMMENT` and one `WITH SYNONYMS`; nothing validates the matches tile the region. `DIMENSIONS (o.d AS o.id COMMENT = 'a' COMMENT = 'b')` → comment `'a'`, `'b'` dropped; `... COMMENT = 'a' banana)` → accepted. Affects every DIMENSIONS/FACTS/METRICS entry and the TABLES tail. **Fix:** track consumed spans; reject uncovered residue.

### P-3 — MED [verified by trace]: OVER clause — `ORDER` without adjacent `BY` silently becomes the frame clause

`src/body_parser/window.rs:187-195`: `BY` is searched anywhere after `ORDER` (junk between them ignored), and if absent there is **no error** — the tail is stored as `frame_clause`. `AVG(m) OVER (PARTITION BY EXCLUDING d ORDER d)` → accepted, `frame_clause = Some("ORDER d")`, no ordering applied. Also: an unquoted ORDER BY dimension named `range`/`rows`/`groups` is claimed by `find_frame_start` (`:333-347`) and becomes a bogus frame clause. **Fix:** hard error on `ORDER` without immediate `BY`; validate frame clause starts with ROWS/RANGE/GROUPS.

### C-5 — MED: DESCRIBE's window-spec rendering drifted from `render_ddl` — drops `frame_clause` entirely

`src/ddl/describe.rs:500-580` vs `src/render_ddl.rs:194-284`: both reconstruct SQL from `WindowSpec`/`non_additive_by`. `render_ddl::emit_window_expr` always emits explicit `NULLS LAST/FIRST` and appends `frame_clause`; describe's inline copy emits `NULLS FIRST` only and **omits `frame_clause`** — `DESCRIBE SEMANTIC VIEW` silently under-reports a metric carrying `RANGE BETWEEN ... PRECEDING AND CURRENT ROW`, and its NULLS rendering contradicts GET_DDL for the same stored object. **Fix:** make `emit_window_expr` (+ a `render_non_additive_by`) `pub(crate)` and call from describe.

### C-2 — MED: `get_ddl` skipped the FF-4/PA-8 name-normalization sweep

Every other single-view read path normalizes through `normalize_view_name` (verified at 8 sites); `src/ddl/get_ddl.rs:94` calls `reader.lookup(name)` raw. `GET_DDL('SEMANTIC_VIEW', 'Orders')` fails while `READ_YAML_FROM_SEMANTIC_VIEW('Orders')` succeeds. Secondary: `get_ddl.rs:109` and `read_yaml.rs:103` parse with bare `serde_json::from_str` instead of `SemanticViewDefinition::from_json`, losing the canonical error context.

### R-3 — MED (user-facing): wildcard errors shoehorned into the wrong error variant

`src/query/table_function.rs:203-250` (three copies) stuffs the wildcard-expansion error text into `ExpandError::EmptyRequest.view_name`, so a failure renders as *"semantic view 'orders: unknown alias ...': specify at least dimensions..."* — the diagnostic buried inside quotes followed by irrelevant advice. **Fix:** add `QueryError::WildcardExpansion { view_name, detail }`; ~15 minutes.

### E-3 — MED [verified mechanism]: textual inlining captures qualified refs on *other* tables

`src/util.rs:170-172` treats `.` as a word boundary, so needle `revenue` matches the column part of `x.revenue`; `inline_facts` only guards the fact's own qualified form. `replace_word_boundary_pairs("x.revenue / 2", [("revenue", "(SUM(o.amount))")])` → `"x.(SUM(o.amount)) / 2"` — invalid SQL, incomprehensible binder error. Third patch-site on the same root cause as SG-3/SG-14 (textual substring scan on un-tokenized SQL); string literals in *other* expressions are also substituted into (mechanism traced, impact SPECULATIVE). **Fix:** one shared, quote-aware, case-insensitive "identifier references in expression" tokenizer used by both validators and inliner — kills E-2/E-3/E-5 as a class.

### C-1 — MED: `list_semantic_views` (the Wave-0 spike) never folded onto the generic scaffolds; its C++ parser is laxer

`cpp/src/shim.cpp:949-1211` carries bespoke structs, a hand-rolled 6-string parse loop, and a duplicate chunked emitter that is line-for-line `sv_emit_varchar_rows`. Concretely divergent: the generic `sv_parse_varchar_payload` rejects trailing bytes (`:1271-1276`); the bespoke list parser has **no trailing-bytes check** (`:1149-1153`) — a wire-format desync every other TF catches loudly passes silently here. **Fix:** rewrite as a `sv_run_varchar_bind` adapter; delete the bespoke copies.

### Smaller confirmed items

- **C-4** — `show_columns.rs:78` says `"Semantic view '...' not found"`, breaking the `view_not_found_msg` canonical-wording invariant every other path honours.
- **P-7 [verified]** — `create_body.rs:156-160` requires literally `FROM YAML` with exactly one space; `FROM  YAML`, `FROM\tYAML`, and `FROM /* fmt */ YAML` (comment blanked to a *run* of spaces) all fail. New instance of the fixed PA-10 class; `match_keyword_prefix` already exists for this.
- **P-6 [verified by trace]** — two dollar-quote grammars: `extract_dollar_quoted` (`create_body.rs:308-330`) accepts any tag (`$1$`), but `blank_sql_comments` (correctly) doesn't recognize it, scans the payload as SQL, and blanks `--`-runs inside the YAML — the corrupted text is then extracted and **stored**. Requires a tag DuckDB itself wouldn't accept, so exposure is low; fix is to share `read_dollar_tag`.
- **P-14 [verified]** — new PA-2 mojibake instances: `clause_bounds.rs:83,136-138` do `bytes[i] as char`; `★` renders as `'â'` in the error message.
- **C-12** — `describe.rs:168-171` `format_json_array` does `format!("\"{s}\"")` with no escaping — a synonym containing `"` yields invalid JSON in DESCRIBE output documented as "JSON array".
- **C-9** — `shim.cpp:2043-2047` defensive null branch would OOB-read if ever taken (`ptr==nullptr ? "" : ptr` with unclamped `len`); make the len 0 too.
- **P-4** — error-caret quality is a lottery: all non-CREATE DDL errors collapse to one fixed position (`rewrite.rs:536-575`); 38/116 `ParseError` sites have `position: None`; entry carets drift left by leading whitespace (`scan.rs:111-141` returns untrimmed offsets with trimmed slices); `metrics.rs:207` components don't add up.
- **E-4** — first-parent bias: `fan_trap.rs:51-56,469-474` and `build_tree_parents` take `parents.first()`; `check_no_diamonds` accepts named multi-parent shapes from *different* sources that `role_playing.rs` doesn't consider role-playing — such a diamond is accepted at CREATE and silently joins through whichever edge was declared first (definitional drift verified; executed wrong-result repro not constructed — SPECULATIVE impact).

---

## 3. Accretion inventory (question a, in detail)

### 3.1 The missing lexer, counted (parser layer)

Keyword matching exists in ≥15 hand-rolled forms (`match_keyword_prefix`, `starts_with_keyword_ci`, `find_keyword_ci`, `find_depth0_keyword`, `find_primary_key`, `find_unique`, `find_non_additive_by_keyword`, `find_partition_by`, `find_partition_by_excluding`, `find_frame_start`, `find_sub_keyword_positions`, inline COMMENT/WITH scans, inline AS checks ×4, SHOW clause checks). Four of those are near-identical copies of "find multi-word keyword with flexible whitespace" differing only in the word list. Quote-state tracking exists in ~8 independent loops. Most tellingly, `extract_view_comment` (`create_body.rs:59-77`) hand-rolls the exact single-quote loop that ST-4 consolidated into `util::extract_single_quoted_prefix` — whose doc says "do not re-inline this logic". The pattern re-grows faster than review rounds prune it.

Consequences visible in this snapshot: fresh instances of all four remediated classes — PA-2 (P-14), PA-5/9 (P-1, P-2, P-3), PA-10 (P-7), TECH-DEBT #24/#25 whitespace-tokenizer (P-11: TABLES *alias* slot, MATERIALIZATIONS name, relationship name/aliases, SHOW `IN`/`FOR METRIC`/`IN SCHEMA` slots, plus two non-quote-aware `.find('(')` at `relationships.rs:65` and `window.rs:50`). Also inconsistent keyword-boundary rules across sites (P-12: `CREATE SEMANTIC VIEW"v"` parses but `LIKE'x'` is rejected; `AS(` legal in MATERIALIZATIONS, illegal after the view name), and `find_keyword_ci` is actually case-sensitive — correct only because all 17 call sites pre-uppercase, one inside a loop (O(n²) on many UNIQUE constraints) (P-13).

### 3.2 Two-pass legacy in `parse/`

`validate_alter` (`rewrite.rs:583-651`) and `rewrite_alter` (`:144-223`) are parallel grammars for the same statement kept in sync by hand (their own comment admits they drifted before PR #50). `plan_ddl` re-runs blanking/trim/prefix-detection on input `plan_rewrite` just processed (R-11); the missing-name check exists in triplicate. An invalid ALTER is fully scanned four times (two runs are DuckDB-imposed; the internal split is self-inflicted). **Fix:** `plan_ddl` returns `ParseError` with positions directly; `validate_alter` should not exist.

### 3.3 Expansion-layer duplication

- **Six copies** of the word-boundary reference scanner (`expand/facts.rs:54-70, 108-124, 337-353, 528-545, 634-649`; `graph/facts.rs:23-53`); a shared helper `references_name` exists and is used by **one of five** in-file sites. Boundary predicates drifted: `util::is_ident_byte` treats ≥0x80 as identifier bytes; local copies at `graph/facts.rs:13-15` and `graph/derived_metrics.rs:417-419` treat them as boundaries (E-5).
- **Four copies** of dimension select-item rendering (scoped-alias rewrite + CAST + `AS quote_ident`), four of FROM-base emission, three of GROUP-BY-ordinals; the `output_type` CAST-wrap `if let` appears ~10×. ~200–250 collapsible lines — but the real payoff is that a shared renderer would have made E-1 impossible (E-6).
- **Three graph traversals** over the same edges (`graph/relationship.rs`+`toposort.rs`; `join_resolver.rs::build_tree_parents`; `fan_trap.rs` adjacency+BFS, its parent-map built twice in-file). One `JoinTree` per expansion would serve all three (E-7).
- `materialization.rs:28-79` vs `:91-128`: the matching algorithm duplicated so `explain` can report the routing name — 45 lines that can drift from the actual routing decision (E-6).
- **13 `fk_columns.is_empty()` legacy guards** for pre-Phase-24 definitions that are mostly unreachable since SG-7 hard-errors on incomplete relationships — the legacy-join era surviving as defensive scar tissue (E-8).
- `{table}__{rel}` scoped-alias format constructed independently at `role_playing.rs:123` and `join_resolver.rs:230` (E-10).

### 3.4 Half-migration inventory (read/FFI side)

| Pattern | Migrated | Not migrated |
|---|---|---|
| ST-2 `run_dispatcher` scaffold | `show_entities` ×6, `show_materializations` ×2, `show_dims_for_metric` ×1 (9/17) | `list` ×2, `describe`, `show_columns`, `get_ddl`, `read_yaml`, `semantic_view` bind, `explain` bind (8/17) — `show_dims_for_metric` proves multi-arg bodies fit, so none are structurally blocked (C-3/R-6) |
| Generic C++ varchar bind/parse/emit | all Wave-1/2 TFs incl. `list_terse` | `list_semantic_views` (bespoke, laxer — C-1) |
| AR-3 self-describing wire format | row payloads | `semantic_view` register payload; C++→Rust LIST(VARCHAR) args (C-6); `wire_len` exists twice |
| ST-1 table-driven registration | Rust side complete | C++ side: 18 five-line wrappers + 7 clone binds; `explain`/`semantic_view` bypass the generic registrar for lack of named-param support, sidestepping the D-05 init-cb invariant (C-7) |
| FF-4 name normalization | 8 of 9 single-view read paths | `get_ddl` (C-2) |
| Canonical `view_not_found_msg` | all read paths + SQL guards | `show_columns` (C-4) |
| `from_json` contextual errors | all TF dispatchers | `get_ddl`, `read_yaml` (C-2) |
| Window/NAB rendering single-source | `render_ddl` | `describe.rs` inline copy, drifted (C-5) |
| ST-4 single-quote extractor | consolidated in `util` | `extract_view_comment` re-inlined it (P-8) |

### 3.5 Confirmed-dead code safe to delete

| Item | Location | Evidence |
|---|---|---|
| `emitted_chunks` field | `shim.cpp:2438` (+`:2689`) | written, never read |
| `util::catch_unwind_to_result` | `util.rs:397-421` | zero callers repo-wide |
| Write-side arms of `function_name` + stale "DDL → function call" header | `parse/rewrite.rs:94-115` | mapped TF names retired in v0.8.0, never registered |
| `extract_ddl_name` (`pub`) | `rewrite.rs:336-416` | zero production callers; a drifted duplicate of `parse_show_filter_clauses` frozen at an earlier hardening level |
| `detect_semantic_view_ddl` + `PARSE_*` consts | `parse/mod.rs:52-55`, `detect.rs:227-233` | pre-parser_override detection API; tests only |
| `skip_leading_whitespace_and_comments` comment branches | `detect.rs:74+` | dead (all five callers run `blank_sql_comments` first) and its doc claim "NOT nested — matches PostgreSQL" is factually wrong (P-5/R-10) |
| `let _ = after_as_offset` | `relationships.rs:60-61` | computed, explicitly discarded |
| unused params `validate_create_body(_query, ...)`, `parse_for_metric(rest, _entity)` | `create_body.rs:89`, `show_clauses.rs:116` | threaded by all callers, never read |
| `sv_count_parser_extensions` | `shim.cpp:2994-3048` | never invoked at runtime; referenced only by a source-text-grepping test — delete or make the test actually call it |

### 3.6 Comment/doc drift recording retired architectures (C-10)

Load-bearing navigation comments describing designs that no longer exist: `list.rs:38-44,203-207` + `shim.cpp:940-947` document the pre-AR-3 headerless wire format; `list.rs:66-68` documents a wrong rc contract (says 2, code returns 1); `native_sql.rs:53-56` references the retired `catalog_conn`; `alter_helpers_ffi.rs:4-6,43-45` says YAML is read "via `read_text(?)`" (replaced in Phase 65.1); `catalog/mod.rs:213-217` says `CatalogReader` wraps a connection "created at extension load time" (false since the per-call borrow model, contradicted three lines later); TECH-DEBT entries 9 and 12 describe shim code that no longer exists (grep-verified zero hits). One sweep commit.

---

## 4. Test coverage (question b, in detail)

### 4.1 What exists (inventory)

~1,090 `#[test]` fns in `src/` (concentrations: `parse/rewrite.rs` 251, `body_parser/mod.rs` 176, `sql_gen.rs` 125). Ten proptest files (~103 tests) including a genuinely hostile round-trip generator (`arb_stored_ident`: quoted idents with whitespace/dots/keywords/non-ASCII/escaped `""`) and a **bind oracle** (`expand_proptest.rs` prepares expanded SQL with `LIMIT 0` against a real schema). Eight fuzz targets, three with real semantic oracles (render fixpoint, quote/paren balance, parse-must-render). 70 sqllogictest files with ~139 `statement error` blocks. And the crown jewel: `test/integration/test_differential.py` — a seeded 4,000-row star schema, ~74 dim×metric combinations executed through both `semantic_view()` and independent hand-written SQL, multiset-compared.

### 4.2 The two verdicts

**Parsers: strong, with narrow residual holes.** Escaped quotes, unicode + byte-offset carets (pinned to exact column in `test_caret_position.py` including a multibyte prefix), case variation, leading comments, trailing commas, keywords-in-string-literals — all covered.

**Output correctness: layered, but the oracle's scope excludes the hardest features.** The differential harness's own SCOPE comment limits it to "supported core" — base-table metrics, ManyToOne, one grain. **Semi-additive, window, wildcard, role-playing USING, and facts requests are validated only by 2–5-row hand-picked sqllogictest fixtures** — exactly the features where ties, NULL ordering, and alias binding produce wrong numbers small fixtures can't catch. E-1 is the existence proof: the semi-additive fixtures have no partition-boundary ties and no dimension whose expr differs from its column, so a silent-wrong-results bug survived the entire suite.

Feature-interaction matrix (any test at all): semi-additive×fan-trap ✅, fan-trap×window ✅, derived×role-playing ✅, fan-trap×derived ~ (differential SG-1) — **all other cells empty**; `phase46_wildcard.test` interacts with nothing.

### 4.3 Prioritized gaps

| ID | Value | Effort | Gap |
|---|---|---|---|
| T-1 | HIGH | M | **Extend `test_differential.py` to semi-additive (with ties at the snapshot date — currently none exist), window (vs hand-written `SUM(...) OVER`), wildcard (`['*']` vs explicit list), and USING.** The harness docstring already mandates this. Would have caught E-1. |
| T-2 | MED | S | Duplicate-clause error path (`clause_bounds.rs:119-126`) has **zero** tests anywhere. One unit + one sqllogictest block. |
| T-3 | MED | S | Empty clause bodies unpinned: `TABLES ()`, `DIMENSIONS ()`, `METRICS ()`, `MATERIALIZATIONS ()` behaviour is undefined-by-test. |
| T-4 | MED | S | Comments *inside* the AS body work only by construction (whole-query blanking); no end-to-end test. Add block-comment-between-clauses and inside-parens cases; pin caret after an in-body comment. |
| T-5 | MED | M | CREATE-route proptests never see hostile clause content: `parse_proptest.rs` uses a fixed canonical body with `[a-z_]` names; the hostile generators live only in `roundtrip_proptest.rs`, entering at `parse_keyword_body` — skipping `plan_rewrite`'s blanking/offset threading/name extraction. Share the generators (`tests/common/`) and drive `plan_rewrite` with rendered hostile defs. The project already learned this lesson once (TC-3) but applied it to one entry point. |
| T-6 | MED | S | Wildcard interaction cells: `['*']` with a PRIVATE metric, a window metric, a semi-additive metric — three distinct routing decisions, no tests. |
| T-7 | LOW | S | Exhaustive 2-clause order inversions; pin (or fix) the `position: None` at `clause_bounds.rs:218`. |
| T-8 | LOW | S | No test parses a large *valid* body (500-entry METRICS, 64-deep parens) — pin linearity against a future accidentally-quadratic scan. |
| T-9 | LOW-MED | L | Rust-side randomized-schema differential proptest. Do T-1 first. |

Plus regression tests for every §2 bug as it's fixed — in particular E-1 needs a dimension-expr-≠-column + tie-at-snapshot fixture, and E-2 needs a mixed-case derived-metric reference.

---

## 5. Rust idiomaticity (question c, in detail)

**Grade: A–.** What's genuinely strong, verified: complete panic containment (every `extern "C"` fn `catch_unwind`-wrapped, including `sv_free_buffer` and the entrypoint with FF-7 post-unwind hardening); type-level FFI contracts (`BorrowedConnection` `#[repr(transparent)]` + `compile_fail` doctest + AST-walking CI guard; `CatalogReader<'a>` with `PhantomData` lifetime); ~10 non-test `unwrap`s in 38k lines, all guarded; RAII where it matters (`PreparedStmt`/`QueryResult`, `Box<[u8]>::into_raw` for the `len == capacity` guarantee, both-or-drop out-pointer contract); enforced single sources of truth (`is_ident_byte`, `DEFINITIONS_TABLE` with a decomposition test, the `sv_registrations!` macro); `Cow` used where it pays (`blank_sql_comments`, length-preserving so carets stay valid); wire serializers that check `u32` overflow instead of clamping.

Findings (beyond clippy-pedantic):

- **R-1 (MED)** — SQL-escaped vs raw strings distinguished only by variable naming (`name_escaped: &str`) through `native_sql.rs` + `catalog/writes.rs`, with an `unescape_sql_arg` inverse confirming both representations share one type. The one place a newtype carries security weight — `SqlLit(String)` with `escape()` constructor makes missed-escape and double-escape compile errors. ~6 signatures.
- **R-2 (MED)** — stringly error core: `Result<_, String>` in ~30 signatures; `plan_rewrite` manufactures positions because inner layers threw them away; the identical `.map_err(|e| ParseError { position: Some(trim_offset + plen), .. })` block pasted 5×; caller contracts as runtime strings (`Err("CREATE forms must use plan_rewrite")`). The crate demonstrably does structured errors well (`ExpandError`/`QueryError`) — the parse half never got the treatment. This is also the root of P-4 (caret lottery): Phase 62 fought hard to restore carets, then the `String`→`ParseError` boundary coarsens them.
- **R-4 (MED)** — 6- and 9-field positional tuples (`MetricEntry`) with `// tuple index 8` comments and a 9-way closure destructuring; fields map 1:1 onto `Metric` — build the struct.
- **R-5 (MED)** — `resolve_names` takes 9 params, 3 of them same-shaped positional error-constructor closures — and the dimension call site **already** passes `DuplicateDimension` in the private-error slot (`sql_gen.rs:282-285`; dead only because `is_private` is hardwired false for dims). Empirical proof the API invites transposition. A small `Resolvable` trait + `EntityKind` collapses it to 3 params.
- **R-6/C-3 (LOW-MED)** — finish the `run_dispatcher` migration (8 hand-rolled dispatchers remain); deletes ~400–500 lines of `match/write_err/return` ladder and shrinks the audited unsafe surface. The "single linear dispatcher" justification comments are disproven by `show_dims_for_metric`, which runs a more complex body under the scaffold.
- **R-7 (LOW)** — `DimensionName`/`MetricName` are 65-line copy-paste twins; `facts: Vec<String>` gets no newtype and silently lacks their case-insensitive semantics. `CiName<K>` with kind markers: one impl, three types.
- **R-8 (LOW)** — parallel positional slices (`resolved_dims` + `dim_scoped_aliases: Vec<Option<String>>`) threaded through three modules and re-indexed `[i]`; zip into `ResolvedDim` and carry a `ResolvedQuery` context struct (signatures shrink from 6 args to 2).
- **R-9 (LOW)** — `ExpandError` ~150 bytes; box the two fat variants, delete the seven `result_large_err` allows.
- **R-13 (INFO)** — init-path `.unwrap()`s (`lib.rs:559,566`) degrade diagnosable failures into "panicked unexpectedly"; `let ... else` with a real message.
- **R-14 (INFO)** — test fixtures spell out all 9–10 struct fields where `..Default::default()` is already used elsewhere in the same files; every `Metric` field addition churns dozens of literals.
- **R-15/R-16 (INFO)** — `Option<&String>` params (`render_ddl.rs:19`); `EmptyRequest` display text drifting between `QueryError` and `ExpandError`.
- **C-11 (LOW)** — serde policy split: `source_table`/`output_type` serialize explicit nulls while sibling fields use `skip_serializing_if`; enum variants persist in Rust casing into stored JSON and YAML export — now a frozen wire format. Not bugs; document the freeze at the top of `model.rs` so the next field-adder doesn't copy at random.

Explicitly checked and clean: no silent error swallowing in production paths (the two `.ok()` in `list.rs` are the documented FF-9 tolerant-listing policy); string building consistently `push_str`/`with_capacity`; `pub(crate)` discipline deliberate; memory ownership across the shim verified leak-free (TECH-DEBT #20's intentional leak is genuinely gone); no C++ exception can cross into a Rust frame; zero Rust statics. One SPECULATIVE note: `SemanticViewGlobalState`'s unsynchronized `Fetch()` cursor relies on DuckDB's default `MaxThreads() == 1` for table functions — true in the pinned version, worth one comment.

---

## 6. From-scratch design sketches and recommended sequence

### 6.1 Parser layer: incremental lexer + cursor migration (recommended)

The grammar is small, regular, and non-recursive except balanced parens. Target: **(1)** a ~250-line lexer producing `Token { kind, span }` over the raw query (kinds: `Ident{quoted}`, `String`, `DollarString`, `Number`, `Symbol`; comments skipped; spans in original bytes) — replaces `blank_sql_comments`, both comment scanners, both dollar-quote grammars, all ~8 quote loops, and makes every caret exact by construction; **(2)** a ~150-line cursor (`peek`, `expect_kw("PRIMARY","KEY")`, `balanced_parens()`, `error_here(msg)`) — replaces the 15 keyword matchers and the offset arithmetic; **(3)** ~800–1,000 lines of recursive-descent clause parsers that consume sequentially — structurally eliminating the P-1/P-2/P-3 silent-discard family ("search for PRIMARY KEY anywhere and slice" becomes unwritable) and the #24/#25 class (identifiers are single tokens whether quoted or not). Expressions stay uninterpreted token runs re-sliced from source — the current design's one genuinely right call. Net ≈ 1,400 lines replacing ~2,900, with strictly better error positions, migrated one clause parser per phase (TABLES first) under the existing sqllogictest/proptest oracle.

### 6.2 Expansion layer: no rewrite; three surgical moves

1. **Fix E-1** (repeat the expression, or two-level CTE).
2. **One reference-scanning engine**: quote-aware, case-insensitive identifier-reference tokenizer shared by graph validators and inliners (kills E-2/E-3/E-5 as a class; `find_fact_references`' dual-buffer approach is the seed).
3. **A ~150-line `SelectSpec` builder** (`items: Vec<SelectItem{expr, cast, alias}>`, `from`, `joins`, `group_by: Ordinals(n)`, optional CTE wrapper) with one `render()`; the four strategies become constructors. The alias-shadowing defense lives in the builder once — E-1 becomes unrepresentable. Then: collapse the materialization matcher to one function returning `Option<&Materialization>`, compute one `JoinTree` per expansion for fan-trap/join-emit/fact-path, and split `sql_gen.rs`'s 4,200 test lines into behaviour-named files (`tests_joins.rs`, `tests_derived.rs`, …) — the phase-named archaeology is what made E-1's blind spot invisible.

### 6.3 Suggested PR sequence

1. **Correctness PR**: E-1 + E-2 + P-1 + P-3 + C-2 + C-5 + R-3 + C-4 + P-7, each with a regression test; extend `test_differential.py` per T-1 in the same PR (it re-catches E-1 independently).
2. **Cheap test holes**: T-2, T-3, T-4, T-6 in one small PR.
3. **Finish the half-migrations**: C-1, C-3/R-6, then the dead-code deletions (§3.5) and the comment sweep (§3.6) — mostly mechanical, high drift-prevention value.
4. **Idiomatic hardening**: R-1 (`SqlLit`), R-2 (structured parse errors + positions at origin — also fixes most of P-4), R-4/R-5 (structs over tuples, trait-based `resolve_names`), P-9 (merge `validate_alter` into `plan_ddl`).
5. **Structural (opt-in, phased)**: 6.2's `SelectSpec` + shared reference engine; then 6.1's lexer migration, TABLES clause first, with T-5's shared hostile generators landed beforehand as the oracle.

---

## 7. Status of the 2026-07-02 review's themes

R3/R4 verifiably landed: CTE-era and legacy-join code fully excised; `sql_throwing` deleted and carets restored (Phase 62); dead OverrideContext FFI retired (AR-7, leak confirmed gone); table-driven registration on the Rust side (ST-1 complete); self-describing row wire format (AR-3, but not extended to the two newer formats — C-6); `schema_version` + upgrade pass with the dead compat fields actually deleted (AR-4/PR-2, clean); quoting single-sourced in `resolution.rs`; the differential harness exists and runs in `test-all`. The recurring meta-lesson from this round: **the remediations that defined an abstraction and migrated every call site stuck; the ones that migrated "enough" sites left exactly the copies where this round's divergences live.** Future remediation PRs should treat "all sites migrated, old form deleted or lint-guarded" as the done criterion, not "scaffold exists and is used somewhere".
