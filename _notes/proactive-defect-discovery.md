# Proactive defect discovery — research & proposal (2026-08-08)

**Status:** proposal for discussion — nothing here is adopted yet. Companion to
`_notes/code-review-2026-08-08.md`. The question it answers: five review rounds have each
found silent wrong-number bugs and the test gaps that admitted them — what could find these
*before* a review round does, and what would stop the recurring classes structurally?

---

## 1. Diagnosis — why bugs keep escaping

Every escape from the last five rounds falls into one of five classes. The classes, not the
individual bugs, are the target.

| Class | Shape | Representative escapes |
|---|---|---|
| **A. Combination-cell wrong numbers** | The engine is correct in every cell a harness randomizes and wrong in a cell pinned at an inert default | EXP-9/10 (`where_clause: None`), EXP-19/20 (derived-over-semi-additive), EXP-26 (COALESCE args), EXP-28 (FACTS requests), PBT-13 (new harness re-pins) |
| **B. Scanner divergence** | N hand-rolled scanners disagree on quotes/comments/escapes; the (N+1)th site is written fresh and wrong | EXP-16/17, PARSE-4/5/7/12/13, IDENT-2 |
| **C. Multi-ingress contract violations** | Two paths into the same model (DDL parse, YAML import) validate differently; renderers assume the stricter path | RT-5/6/7/8/9, MODEL-1 |
| **D. Vacuous / self-disabling tests** | Assertions that cannot fail, early-return escapes, first-failure masking, degraded pins whose ledger entry expired | TC-12/13/14, RT-5's fuzz oracle, the `data_type` column |
| **E. Whitelist patches** | A fix enumerates the spellings of a hazard instead of guarding the class; the next spelling walks through | EXP-21→25/26 (constant whitelist), EXP-12→PARSE-8→EXP-30 (`eq_ignore_ascii_case` sweeps), EXP-23→27 (join added, fence not consulted) |

Two structural observations drive everything below:

1. **Hand-formulated oracles don't scale to the combination space.** The differential
   harnesses work — every numeric oracle we've written has held — but each oracle must be
   *hand-derived per feature*, so coverage grows linearly while the feature matrix grows
   combinatorially. Role-playing (PBT-10) has stayed uncovered for three rounds not out of
   neglect but because its independent oracle is genuinely hard to formulate. The fix is a
   class of oracle that doesn't need formulating (§2.1).
2. **Discipline that lives in review vigilance decays; discipline that lives in CI doesn't.**
   Confirm-the-red held in #203–#209 *because agents followed CLAUDE.md*, yet PBT-13 landed in
   the same commits — the same authors, the same week, the rule already written down. Rules
   that survive are the ones a machine checks (TEST_LIST sync via `just check-test-list` has
   never regressed since it became a CI check).

## 2. Proposed techniques

Ordered within each subsection by leverage. §3 gives the adoption sequence.

### 2.1 Self-checking (metamorphic) oracles — the highest-leverage addition

The insight from the SQLancer line of work (TLP, NoREC, PQS — see
https://github.com/sqlancer/sqlancer and the TLP paper,
https://www.manuelrigger.at/preprints/TLP.pdf): you can detect logic bugs **without knowing
the correct answer**, by checking that two query formulations that *must* agree, do. Because
no hand-formulated oracle is needed, generation is unconstrained — these harnesses can range
over role-playing, hostile identifiers, arbitrary topologies and every request-shape
combination, exactly the cells our differential harnesses cannot reach. Four families, in
order of fit:

**(a) Definition algebra** — for any derived metric `d = f(m1, …, mk)`, querying `d` must
equal computing `f` over the separately-queried `m1…mk` (same dimensions). Likewise a window
metric vs. its inner metric's per-partition aggregate, and a metric queried alone vs. queried
alongside others. This is pure self-consistency — `double_balance == 2 × balance` — and it
catches the EXP-19/20/24 class *generically*: any place where inlining, snapshot routing, or
re-anchoring treats a composed metric differently from its components. Cheap to implement
(drive `expand()` twice, compare in DuckDB), applies to every generated model.

**(b) Roll-up consistency** — for additive metrics, the total-query result must equal the
sum over any grouped query's rows (`SUM(m) over all == Σ groups of (m BY d)`), and COUNTs must
add. Any fan-out duplication, phantom row, or grain-substitution error breaks additivity
somewhere; this catches the EXP-11/21/25/26 class without knowing the right number, only that
the two aggregations must agree. (Semi-additive metrics assert the same *within* the allowed
dimensions.)

**(c) TLP over `where_clause`** — partition any generated predicate `p` into `p`,
`NOT p`, `p IS NULL`: the three filtered results must recombine to the unfiltered result
(sums add, counts add). This exercises predicate placement in every CTE topology — the
EXP-13/14/22/27 class — with the predicate itself randomly generated rather than mirrored
into a hand-written oracle.

**(d) Data metamorphism** — mutate the *data* in ways with a known effect and assert the
delta: inserting a childless parent row must leave every child-grain metric and every FACTS
result unchanged (this is *precisely* the EXP-21/25/26/28/29 invariant); duplicating a
dimension row that nothing joins to must change nothing; inserting a NULL-keyed child row
must change only NULL-group cells. Each such rule is one generator tweak plus one assertion.

Concretely: one new harness, `tests/metamorphic_proptest.rs`, generating models over the
existing `tests/common` builders (crucially *without* the oracle-imposed pins — role-playing
edges, hostile identifiers, and FACTS requests become generatable on day one), running
families (a)–(d) as separate properties. Rough effort: (a)+(b) a day or two; (c) another
day; (d) incremental afterwards. Would have caught, by inspection of the historical list:
EXP-9/10/11/19/20/21/22/24/25/26/27/28/29 — thirteen of the wrong-number findings across
three rounds.

### 2.2 One conformance pipeline for the multi-ingress contract (class C)

Today the round-trip oracles each cover a segment (`roundtrip_proptest` parses via
`parse_keyword_body` directly; `yaml_proptest` checks serde symmetry; the fuzz target checks
render→parse). RT-7/8 and MODEL-1 lived in the seams *between* segments — front-door comment
blanking, fields with no DDL emission, validations run on one path only.

Proposal: a single canonical property — from **either** ingress (generated DDL or generated
YAML), build the model, then assert the full loop: `model → render_ddl → front-door parse
(including `blank_sql_comments` preprocessing, i.e. the *real* CREATE path) → model′ ≡ model`,
and `model → render_yaml → yaml import → model″ ≡ model`. Run it as both a proptest and a
fuzz target (the corpus transfers). Any field added to `model.rs` is then automatically under
contract: forget the emit site or the validation twin and the property fails. This
structurally closes class C rather than patching its instances — and it is the check that
makes MODEL-1-style "validated on one path only" impossible to reintroduce silently.

### 2.3 Mutation testing for vacuous-test detection (class D)

`cargo-mutants` (https://mutants.rs/, `--in-diff` documented at
https://mutants.rs/in-diff.html and https://mutants.rs/pr-diff.html) injects source mutations
and reports every mutant no test kills — which is exactly a machine-checked version of the
confirm-the-red rule, run continuously instead of at fix time. A surviving mutant in
`facts.rs`'s guard predicate *is* the "this test would pass anyway" signal that TC-12's
assertion-free arms and TC-14's early-return escape embody.

Practical shape for this codebase (~60k lines, slow proptests):
- **PR gate:** `cargo mutants --in-diff <base-diff>` so only changed code is mutated —
  minutes, not hours; a missed mutant in the diff is a review flag, not necessarily a merge
  block at first.
- **Nightly/weekly:** full run scoped to the correctness core (`--file src/expand/*.rs
  --file src/parse/*.rs --file src/body_parser/*.rs ...`), with `PROPTEST_CASES` lowered
  (e.g. 16) via `[mutants]` config so the proptest suites stay useful as killers without
  dominating runtime; publish the outcome list as an artifact and triage new survivors.
- Calibrate timeouts once (`--timeout-multiplier`); exclude FFI-gated code that can't run
  under `cargo test`.

### 2.4 Hazard-pattern lints (classes B and E)

The project keeps re-fixing the same *pattern* at new sites. Ban the pattern mechanically:

- **clippy `disallowed-methods`** (clippy.toml): `str::eq_ignore_ascii_case` disallowed with
  the message "identifier comparison must go through `ident::ident_matches`; keyword
  comparison sites get `#[allow]` with a comment". This turns the PARSE-8/EXP-12/EXP-30
  whack-a-mole into a compile-time rule; the existing keyword sites get explicit,
  greppable allows.
- **Scanner-routing meta-test:** a unit test that greps `src/` for the byte-level scanning
  primitives (`b'\''`, `b'"'`, `b'$'` adjacency loops) outside the four blessed scanner
  modules, failing with "route through `QuoteState`/`blank_sql_comments`" — the same
  mechanism as `no_long_lived_conn.rs`'s allow-list, which has worked. New scanners can
  still be written; they just can't be written *silently*.
- Same mechanism for `format!("... LIKE '{}'", …)`-shaped predicate splicing (IDENT-6's
  class) and FFI `unwrap_or(<rc>)` panic swallowing (FF-16's class).

### 2.5 Generator-axis ledger and pin waivers (class A's recurrence)

PBT-13 shows the pin pattern re-enters through new files. Two cheap guards:

- **Pin waivers:** a CI grep (a unit test in `tests/common`) over `tests/*_proptest.rs` for
  the known-inert literals (`where_clause: None`, `facts: vec![]`, `output_type: None`, …)
  requiring a `// PIN: <reason>, TECH-DEBT #<n>` on the same line. A pin is then a *decision
  with a ledger entry*, not a default. (The grep is crude but the false-positive cost is one
  comment; precision can come later.)
- **Axis ledger:** promote the review's pinned-field matrix into a checked-in table
  (TECH-DEBT.md "Test Coverage Gaps" — currently stale per TC-15) regenerated or at least
  verified by that same meta-test, so "which harness varies which axis" is a maintained
  artifact rather than something each review reconstructs. Adding a field to `QueryRequest`
  without touching the ledger fails the meta-test (an exhaustive struct-literal in the test
  makes the compiler enforce field-awareness).

### 2.6 Fix-audit checklist (class E, the EXP-27 lesson)

EXP-27 happened because a fix *added a capability* (a new joinable table source) without
enumerating the invariant guards that assume the old capability set. One paragraph in
CLAUDE.md's discipline section:

> **When a change adds a capability, list the fences.** Any change that makes a new table
> reachable (join emission, reference inlining, source_tables), accepts new syntax, or adds
> an ingress path must name, in the PR description, each existing guard that assumes the old
> set (fan-trap checks, scanner set, validation choke points) and say for each: extended, or
> why not applicable. "The fence didn't know about the new edge" is now the second fix-round
> regression (EXP-23→27; PARSE-7→12) — the checklist is cheaper than the round-trip.

Process text, not tooling — but it's the class E counterpart of confirm-the-red, and it costs
nothing.

### 2.7 sqllogictest halt-masking (class D)

The runner stops a file at its first failure, so N cases yield one observed red (documented
in CLAUDE.md, still relies on care). Mitigations, cheapest first: (i) convention — regression
`.test` files hold one scenario each (the `cr*` files are already close); (ii) a
`just confirm-red <test> <ref>` recipe that checks out `<ref>`'s `src/` into a temp worktree,
builds, and runs the named test expecting failure — mechanizing the revert-and-watch step the
#204/#207 audits did by hand; (iii) if the runner is ours, a continue-on-error mode with
per-statement reporting (worth a look at what `sqllogictest-rs` upstream supports before
building anything).

### 2.8 Catalog DDL state-machine testing (CAT class)

`proptest-state-machine` (part of the proptest project) drives random operation sequences
against a system under test with a reference model. A model of "map from (schema, name) to
definition" with operations CREATE / CREATE OR REPLACE / ALTER RENAME / DROP / DROP SCHEMA /
re-open, checked against the real catalog after every step, would have caught CAT-5 (case
respelling), CAT-6 (schema-drop orphans) and CAT-8 (RO companion-file) as invariant
violations rather than review finds. Medium effort; needs the file-backed harness the
catalog unit tests already use.

### 2.9 Snowflake parity fixtures (reference-differential)

No emulator covers semantic views: fakesnow and the Go/DuckDB emulators mock
warehouse/DDL surface, not `CREATE SEMANTIC VIEW` (checked 2026-08-08 — no evidence of
support; unsurprising given the feature's age). Pragmatic path: a curated, versioned fixture
set — real Snowflake DDL + `SEMANTIC_VIEW()` queries + captured results over tiny data —
run manually against a trial account when parity questions arise, replayed in CI against us
always. This converts "Snowflake does X" from a research task during review into a pinned
test, one fixture at a time, starting with the behaviours TECH-DEBT already records as
divergences.

### 2.10 Coverage in CI — supporting signal only

`cargo-llvm-cov` region coverage with a diff-coverage report on PRs ("changed lines with no
test execution") is cheap to add and catches the grossest gaps, but note that **coverage
would not have caught most of this project's escapes** — the wrong-number bugs execute fine
under tests that assert the wrong thing or nothing. Useful as a floor, not as the strategy;
mutation testing (§2.3) is the version of this signal that actually matches our failure mode.

## 3. Suggested adoption order

| Step | What | Effort | Classes | Would have caught (historical) |
|---|---|---|---|---|
| 1 | §2.6 fix-audit checklist + §2.4 lints + §2.5 pin waivers | hours | B, E, A-recurrence | EXP-27, EXP-30, PARSE-12 (lint sweep), PBT-13 |
| 2 | §2.1(a)+(b) definition-algebra + roll-up metamorphic harness | 1–2 days | A | EXP-9/10/11/19/20/21/24/25/26 |
| 3 | §2.3 `cargo mutants --in-diff` on PRs | ~1 day | D | TC-12/14 shapes, future vacuous fixes |
| 4 | §2.2 conformance pipeline property + fuzz target | 2–3 days | C | RT-5/6/7/8/9, MODEL-1 |
| 5 | §2.1(c)+(d) TLP + data metamorphism | 1–2 days | A | EXP-13/14/22/27/28/29 |
| 6 | §2.7 confirm-red automation; §2.3 nightly full mutants | 1–2 days | D | — (hardens process) |
| 7 | §2.8 catalog state machine; §2.9 parity fixtures | ~1 wk each | CAT, parity | CAT-5/6/8, PAR-* |

Steps 1–3 fit alongside the current fix round; step 2 is the single highest bugs-per-effort
item on the list and is the one that finally gives role-playing (PBT-10) and hostile-identifier
numerics (PBT-12) randomized coverage, because self-checking oracles don't need the
hand-formulated oracle those cells were waiting on.

## 4. What already works — keep it

The differential-oracle harness family (every hand-written oracle has held; harness gaps, not
oracle errors, admitted the bugs), confirm-the-red as written, the anti-vacuity counter
pattern, TEST_LIST/fuzz-list CI sync, the multi-agent review cadence (five rounds, every
round paying for itself), and the TECH-DEBT ledger *when entries actually land* — the
process gap there is recording latency, which step 1's waiver rule addresses at the test
layer and §2.6 at the fix layer.

## Sources

- SQLancer (TLP / NoREC / PQS): https://github.com/sqlancer/sqlancer;
  TLP paper: https://www.manuelrigger.at/preprints/TLP.pdf;
  PQS paper: https://www.usenix.org/system/files/osdi20-rigger.pdf
- cargo-mutants: https://mutants.rs/ (diff-scoped runs: https://mutants.rs/in-diff.html,
  https://mutants.rs/pr-diff.html; repo: https://github.com/sourcefrog/cargo-mutants)
- Snowflake emulators surveyed for semantic-view support (none found):
  https://github.com/nnnkkk7/snowflake-emulator and fakesnow (PyPI) — warehouse-surface
  mocks only.
