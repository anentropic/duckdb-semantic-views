# Persona Report

**Generated:** 2026-05-06
**Audience:** Data engineers exploring semantic views (intermediate)
**Scenarios tested:** 5 (incremental refresh — v0.8.0 transactional DDL)
**Results:** 4 PASS, 1 PARTIAL, 0 FAIL

## Summary

The transactional-DDL refresh reads cleanly from this persona's perspective. The new `docs/explanation/transactional-ddl-and-limitations.rst` page is well structured: the headline change is clear in the first 100 words, the "skip if you're a single-process user" framing matches how the persona evaluates limitations, and the worked Python try/except pattern for the `IF NOT EXISTS` race is exactly the level of guidance an intermediate engineer needs. Notes on the affected reference pages (CREATE / DROP / ALTER / DESCRIBE / SHOW / yaml-definitions) all cross-link back consistently, and the "Since v0.8.0" framing is applied uniformly across alter/drop. No dangling references to the removed "caret loss" passage were found, and the removed "16-DB LRU" passage left no readable gap (no remaining text suggests a database-count cap). The single PARTIAL is a genuine usability gap, not a writing defect: the Snowflake comparison page does not yet mention transactional-DDL parity in its concept-mapping table or "Key Differences" section, which is the first place a Snowflake-experienced reader will look when forming expectations about `BEGIN ... ROLLBACK` behaviour.

---

## Scenario S1: I want to wrap a CREATE SEMANTIC VIEW in BEGIN/ROLLBACK and have it actually roll back

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - No prominent "transactional DDL" callout on the homepage, but the site has clearly-labelled DDL reference and Explanation cards. As an intermediate user looking for transaction semantics I head for either reference/CREATE or explanation.
   - Followed: "DDL reference" card → `docs/reference/create-semantic-view.rst`.
2. Navigated to: `docs/reference/create-semantic-view.rst`
   - Found: a `.. note::` directly after the Statement Variants section that says "Since v0.8.0 all four CREATE body variants participate in your surrounding transaction. BEGIN ... ROLLBACK discards an uncommitted CREATE." with a `:ref:` link to `explanation-transactional-ddl`.
   - Followed: ref link → `docs/explanation/transactional-ddl-and-limitations.rst`.
3. Navigated to: `docs/explanation/transactional-ddl-and-limitations.rst`
   - Found: section "DDL Now Participates in Your Transaction" with three concrete `BEGIN ... ROLLBACK` examples for CREATE, DROP, and ALTER respectively, plus an explicit statement that this covers all four CREATE body variants and the `OR REPLACE` / `IF NOT EXISTS` modifiers.
   - Type-alignment check: I needed an explanation ("does this work and how do I rely on it?") and that's exactly what this page provides. Good fit.

### Outcome

The user can confidently wrap CREATE in `BEGIN/ROLLBACK` after reading this. The example is copy-pasteable, the prose explicitly contrasts pre-v0.8.0 behaviour, and the "you can simplify" note is reassuring without being preachy.

---

## Scenario S2: I tried to DROP a view that doesn't exist — what error do I see and how should I handle it?

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst`
   - Followed: "DDL reference" → reference index → `docs/reference/drop-semantic-view.rst`.
2. Navigated to: `docs/reference/drop-semantic-view.rst`
   - Found: Statement Variants block clearly distinguishes `DROP SEMANTIC VIEW <name>` (errors if missing) from `DROP SEMANTIC VIEW IF EXISTS <name>` (silent no-op).
   - Found: a `.. note::` with the v0.8.0 framing, and crucially the new concurrent-drop guard text: `semantic view '<name>' was concurrently dropped`. Good — the persona was wondering about this exact scenario.
   - Followed: `:ref:` link to `explanation-transactional-ddl`.
3. Navigated to: `docs/explanation/transactional-ddl-and-limitations.rst` → "DROP and ALTER Without IF EXISTS Detect Concurrent Drops"
   - Found: clear rationale ("you asked for an operation on a specific view, the view was there when the extension checked, and then it wasn't") and the explicit contract for `IF EXISTS`.
4. Cross-checked: `docs/reference/error-messages.rst`
   - I would expect this catalogue to include the new `was concurrently dropped` error since it is a user-facing message users will grep for.
   - Found: the page lists DDL errors, materialization errors, YAML errors, query errors, wildcard errors, near-miss detection. The concurrent-drop error is **not** indexed here.
   - This is borderline — the persona will most likely reach the explanation page via the `DROP` reference's note, which is the primary path. But a user who hit the error in the wild and pasted it into a Ctrl+F-equivalent search of the error catalogue would not find it. Recording as a minor observation, not a blocker. The two reference pages (DROP, ALTER) and the explanation page all carry the literal error string, so any reasonable search lands somewhere useful.

### Outcome

The persona understands both the missing-view (`does not exist`) error and the concurrent-drop (`was concurrently dropped`) error, and gets the `IF EXISTS` contract for both. PASS.

### Minor observation (not a verdict driver)

The `Error Messages` reference page does not yet have an entry for `semantic view '<name>' was concurrently dropped`. That is the canonical "look up an error string" page. Consider adding a short entry under DDL errors. This is **not** a FAIL or PARTIAL because the error message string is reachable via at least three other pages (drop, alter, explanation) and the path through the reference DDL pages is the more natural one for this persona.

---

## Scenario S3: I want to ALTER a view's name as part of a multi-statement migration that may roll back

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst` → reference index → `docs/reference/alter-semantic-view.rst`.
2. Navigated to: `docs/reference/alter-semantic-view.rst`
   - Found: Statement Variants block enumerates RENAME TO, SET COMMENT, UNSET COMMENT in both with-IF-EXISTS and without forms.
   - Found: a `.. note::` immediately after Statement Variants, reading: "Since v0.8.0 ALTER participates in your surrounding transaction (BEGIN ... ROLLBACK restores the previous name and comment). Since v0.8.0, the non-IF EXISTS forms additionally raise `semantic view '<name>' was concurrently dropped` ..."
   - The wording "restores the previous name and comment" is concrete — it directly answers "if I ROLLBACK my migration, does the rename undo?"
   - Followed: ref link → explanation page.
3. Navigated to: `docs/explanation/transactional-ddl-and-limitations.rst`
   - Found: in "DDL Now Participates in Your Transaction" the third example is exactly `BEGIN; ALTER ... RENAME TO ...; ROLLBACK; -- the view is still called order_metrics`. Direct match.

### Outcome

The persona has full confidence the rename participates in transactions. The Snowflake-experienced reader will note that this matches Snowflake's behaviour without needing a comparison table to spell it out (because it just works the way DDL is "supposed" to work). PASS.

### Cross-page consistency check

The "Since v0.8.0" wording in `alter-semantic-view.rst` matches the wording in `drop-semantic-view.rst` and `create-semantic-view.rst` exactly. Both reference pages correctly use 0.8.0 (not 0.8.1) and link to the same explanation anchor. Consistent.

---

## Scenario S4: I just CREATEd a view in a transaction; can my next DESCRIBE inside the same transaction see it?

**Verdict:** PASS

### Navigation Path

1. Started at: `docs/index.rst` → reference index → `docs/reference/describe-semantic-view.rst`.
2. Navigated to: `docs/reference/describe-semantic-view.rst`
   - Found: a `.. note::` directly under Parameters: "DESCRIBE SEMANTIC VIEW reads committed catalog state. A CREATE / ALTER / DROP issued in the same uncommitted transaction is not yet reflected here -- commit first, then describe."
   - Followed: ref link to `explanation-txn-ddl-write-visibility`.
3. Navigated to: `docs/explanation/transactional-ddl-and-limitations.rst` → "Reads Inside an Open Transaction See Committed State"
   - Found: a worked SQL example showing exactly the scenario from the goal:
     ```sql
     BEGIN;
     CREATE SEMANTIC VIEW v ...;
     SHOW SEMANTIC VIEWS;   -- v is NOT in the result yet
     COMMIT;
     SHOW SEMANTIC VIEWS;   -- now v is listed
     ```
   - Plus an additional bonus paragraph about `semantic_view(...)` query reads also seeing committed state (intermediate users who try to insert + query in the same transaction will immediately hit this).
   - Plus a forward-looking line "This limitation will go away when DuckDB exposes the hook the extension needs," which is appropriate calibration for an intermediate audience evaluating maturity.

### Outcome

The persona has no remaining confusion. The DESCRIBE / SHOW pages flag the limitation, the explanation page demonstrates it concretely, and the rule of thumb ("commit before introspecting") is explicit. PASS.

### Cross-page consistency check

The `describe-semantic-view.rst` note links to `:ref:explanation-txn-ddl-write-visibility` (a sub-anchor), not the page top — sharper landing than the more general drop/alter notes. `show-semantic-views.rst` carries the same visibility note with consistent wording. Good.

---

## Scenario S5: I'm comparing this extension to Snowflake's transactional DDL — does it match?

**Verdict:** PARTIAL

### Navigation Path

1. Started at: `docs/index.rst`
   - Followed: "Snowflake comparison" card → `docs/explanation/snowflake-comparison.rst`.
2. Navigated to: `docs/explanation/snowflake-comparison.rst`
   - Looked at: "Concept Mapping" table. Has rows for Define / Tables / Relationships / Dimensions / Metrics / Facts / Derived metrics / Semi-additive / Window / Metadata / Access modifiers / Materializations / Query interface / Wildcards / DESCRIBE / SHOW / Terse SHOW / SHOW COLUMNS / IN SCHEMA / GET_DDL / ALTER / DROP. **No row for transactional DDL.**
   - Looked at: "Key Differences" section. Subsections cover Primary Key Declarations, Query Interface, Cardinality Inference, USING RELATIONSHIPS, Facts Query Mode, Semi-Additive and Window Metrics, Materializations. **No subsection mentions transactional DDL or read-committed visibility.**
   - Looked at: "Features Not Yet Supported" — three rows, none about transactions.
   - Looked at: bottom of page for any cross-link to `explanation-transactional-ddl`. None.
3. Backtracked to: `docs/explanation/index.rst`
   - The new bullet for transactional-ddl is present and clear: "How transactional DDL works since 0.8.0 and the small set of caveats around read visibility, the PEG parser, and concurrent writers."
   - So the persona can find the page from the Explanation index, but only by leaving the Snowflake-comparison page first.
4. Navigated separately to: `docs/explanation/transactional-ddl-and-limitations.rst`
   - Found the full picture, but it isn't framed as a Snowflake comparison. The persona has to hold "what does Snowflake do?" in their head and infer alignment from the description.

### Gap Analysis

**Where:** `docs/explanation/snowflake-comparison.rst`, both the Concept Mapping table (around lines 26-99) and the Key Differences section (lines 154-356).

**What:** No mention of transactional DDL parity. A Snowflake-experienced reader doing a feature-by-feature comparison ahead of an evaluation/migration will look here first to confirm `BEGIN ... ROLLBACK` behaves the same way it does in Snowflake. They won't find an answer on this page. The information exists (and reads well) on the new transactional-ddl page, but the user has to navigate away from the comparison page to discover it. Type-alignment is fine on each page individually — the issue is a missing cross-reference between two explanation pages.

**Impact:** PARTIAL, not FAIL, because:
- The transactional-ddl explanation page is reachable in two clicks via the Explanation index, and exists on the homepage's Explanation toctree.
- A determined evaluator will find it.
- But the "compare features and decide if it fits" task in the persona's `user_tasks` list is a primary use case, and an evaluator who reads only the Snowflake comparison and decides on that basis will leave with an incomplete picture — they may still believe the extension's DDL is non-transactional (which was true before v0.8.0), since the comparison page doesn't say otherwise.

**Suggested Fix:** Two options, either is sufficient:

1. **Minimal:** add one row to the Concept Mapping table:
   - Concept: "Transactional DDL"
   - Snowflake column: "DDL participates in BEGIN/COMMIT/ROLLBACK"
   - DuckDB column: "DDL participates in BEGIN/COMMIT/ROLLBACK (since v0.8.0); see :ref:\`explanation-transactional-ddl\` for read-visibility caveat and concurrent-writer notes"

2. **Better:** add a short subsection to "Key Differences" titled "Transactional DDL" that says, in two or three sentences, "Both systems make CREATE/DROP/ALTER SEMANTIC VIEW transactional. Read visibility within an open transaction differs slightly: introspection commands (DESCRIBE, SHOW) see committed state only — see :ref:\`explanation-transactional-ddl\` for details." Snowflake-experienced readers will appreciate the explicit callout that introspection inside a transaction does not see uncommitted writes — that is a behavioural divergence worth flagging up-front.

Option 2 is preferred because read-committed-visibility is a genuine behavioural divergence from Snowflake (where DESCRIBE / SHOW see uncommitted DDL inside the same transaction), not a parity item. The existing transactional-ddl page acknowledges this is a temporary limitation pending a DuckDB hook, but the comparison page never tells the Snowflake user it exists.

---

## Specific verifications requested

### "Caret loss" passage removal

Verified clean. I read the full `transactional-ddl-and-limitations.rst` end to end and checked for trailing references — there is no remaining sentence that begins to discuss caret formatting and trails off, no "as discussed above" pointing at deleted material, and no introduction-summary mismatch (the Summary section on the explanation page lists exactly four bullets that match the four kept sections: rollback, read-committed visibility, IF NOT EXISTS race, PEG parser). The error-messages reference page never discussed caret rendering in the first place, so there is nothing dangling there either. A persona reading either page will not wonder "wait, what about caret formatting?".

### "16-DB LRU" passage removal

Verified clean. The transactional-ddl page mentions multi-process / multi-connection scenarios in three places (CREATE race, DROP/ALTER race, summary), and in each case the framing is "if multiple processes are issuing DDL against the same database file at the same time" — never "if you have many databases attached." There is no residual sentence that hints at a database-count cap, and an intermediate persona reading the page comes away with the correct mental model: the extension handles arbitrary numbers of attached databases; the only multi-something caveats are about multi-process writers, which are flagged as "mostly theoretical for typical DuckDB usage." Good removal.

### "Since v0.8.0" framing consistency

Verified consistent across all four pages that carry the framing:

| Page | "Since v0.8.0" wording present? | Links to explanation? |
|------|--------------------------------|----------------------|
| `docs/reference/create-semantic-view.rst` | Yes — "Since v0.8.0 all four CREATE body variants participate ..." | Yes (`:ref:explanation-transactional-ddl`) |
| `docs/reference/alter-semantic-view.rst` | Yes — "Since v0.8.0 ALTER participates ... Since v0.8.0, the non-IF EXISTS forms additionally raise ..." | Yes |
| `docs/reference/drop-semantic-view.rst` | Yes — "Since v0.8.0 DROP participates ... Since v0.8.0, DROP SEMANTIC VIEW (without IF EXISTS) additionally raises ..." | Yes |
| `docs/how-to/yaml-definitions.rst` | Yes — "Since v0.8.0 CREATE SEMANTIC VIEW ... FROM YAML FILE participates ..." | Yes |
| `docs/explanation/transactional-ddl-and-limitations.rst` | Yes — `.. versionadded:: 0.8.0` directive at top, and "Before v0.8.0 these statements committed independently of the surrounding transaction." | n/a (this is the page) |
| `docs/explanation/index.rst` | Yes — "How transactional DDL works since 0.8.0 ..." | Yes (toc bullet) |

No 0.8.1 references remain. The transition from the prior 0.8.1 framing to 0.8.0 is fully and consistently applied.

---

## Revision Recommendations

### FAIL Issues (trigger revision)

None. No scenario failed.

### PARTIAL Issues (for project author approval)

| Scenario | Page | Gap | Suggested Fix |
|----------|------|-----|---------------|
| S5 | `docs/explanation/snowflake-comparison.rst` (Concept Mapping table and Key Differences section) | Snowflake comparison page does not mention transactional DDL at all, so a Snowflake-experienced reader doing a feature-by-feature evaluation will not learn that v0.8.0 made DDL transactional, nor that read-committed visibility inside an open transaction is a behavioural divergence from Snowflake. | In `docs/explanation/snowflake-comparison.rst`, Key Differences section: add a short "Transactional DDL" subsection (2-3 sentences) noting parity for CREATE/DROP/ALTER under BEGIN/ROLLBACK, the read-committed-visibility divergence for DESCRIBE/SHOW inside an open transaction, and a `:ref:\`explanation-transactional-ddl\`` link. Optionally also add a "Transactional DDL" row to the Concept Mapping table. |

### Minor observation (not a verdict driver, no action required)

`docs/reference/error-messages.rst` does not include an entry for `semantic view '<name>' was concurrently dropped`. The error string is documented on three other pages (drop, alter, transactional-ddl explanation), so users hitting it will find guidance. If/when the error catalogue is next refreshed, consider adding a short entry under DDL errors for completeness. Not blocking this milestone.
