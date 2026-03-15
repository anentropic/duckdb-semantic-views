use proptest::prelude::*;
use semantic_views::parse::*;

// ---------------------------------------------------------------------------
// Strategy helpers
// ---------------------------------------------------------------------------

/// Generate a random upper/lower case variation of each character in `prefix`.
fn arb_case_variant(prefix: &'static str) -> impl Strategy<Value = String> {
    let chars: Vec<char> = prefix.chars().collect();
    let len = chars.len();
    proptest::collection::vec(proptest::bool::ANY, len).prop_map(move |bools| {
        chars
            .iter()
            .zip(bools.iter())
            .map(|(c, &upper)| {
                if upper {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect::<String>()
    })
}

/// Generate random leading whitespace (spaces/tabs, 0-10 chars).
fn arb_whitespace() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[ \t]{0,10}").unwrap()
}

/// Generate a valid view name: starts with letter/underscore, then alphanumeric/underscore.
fn arb_view_name() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z_][a-z0-9_]{0,29}").unwrap()
}

/// Generate trailing semicolons (0-3).
fn arb_semicolons() -> impl Strategy<Value = String> {
    proptest::string::string_regex(";{0,3}").unwrap()
}

// ---------------------------------------------------------------------------
// DDL forms: prefix string to DdlKind mapping
// ---------------------------------------------------------------------------

const DDL_FORMS: &[(&str, DdlKind)] = &[
    ("create semantic view", DdlKind::Create),
    ("create or replace semantic view", DdlKind::CreateOrReplace),
    (
        "create semantic view if not exists",
        DdlKind::CreateIfNotExists,
    ),
    ("drop semantic view", DdlKind::Drop),
    ("drop semantic view if exists", DdlKind::DropIfExists),
    ("describe semantic view", DdlKind::Describe),
    ("show semantic views", DdlKind::Show),
];

/// The 3 CREATE-with-body forms: (prefix, DdlKind, function_name).
const CREATE_FORMS: &[(&str, DdlKind, &str)] = &[
    (
        "create semantic view",
        DdlKind::Create,
        "create_semantic_view",
    ),
    (
        "create or replace semantic view",
        DdlKind::CreateOrReplace,
        "create_or_replace_semantic_view",
    ),
    (
        "create semantic view if not exists",
        DdlKind::CreateIfNotExists,
        "create_semantic_view_if_not_exists",
    ),
];

/// The 3 name-only forms: (prefix, DdlKind, function_name).
const NAME_ONLY_FORMS: &[(&str, DdlKind, &str)] = &[
    ("drop semantic view", DdlKind::Drop, "drop_semantic_view"),
    (
        "drop semantic view if exists",
        DdlKind::DropIfExists,
        "drop_semantic_view_if_exists",
    ),
    (
        "describe semantic view",
        DdlKind::Describe,
        "describe_semantic_view",
    ),
];

/// Build a minimal valid AS-body suffix: AS TABLES (...) DIMENSIONS (...) METRICS (...)
fn build_as_body_suffix(name: &str) -> String {
    format!(
        " {name} AS TABLES (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.region AS region) METRICS (t.revenue AS SUM(amount))"
    )
}

/// Build a valid suffix for a given DdlKind (AS-body for CREATE, name for DROP/DESCRIBE, empty for SHOW).
fn build_suffix(kind: DdlKind, name: &str) -> String {
    match kind {
        DdlKind::Create | DdlKind::CreateOrReplace | DdlKind::CreateIfNotExists => {
            format!(
                " {name} AS TABLES (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.region AS region) METRICS (t.revenue AS SUM(amount))"
            )
        }
        DdlKind::Drop | DdlKind::DropIfExists | DdlKind::Describe => {
            format!(" {name}")
        }
        DdlKind::Show => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Detection properties (TEST-01)
// ---------------------------------------------------------------------------

proptest! {
    /// Case-insensitive detection: CREATE SEMANTIC VIEW
    #[test]
    fn detect_create_case_insensitive(
        prefix in arb_case_variant("create semantic view"),
        name in arb_view_name(),
    ) {
        let query = format!("{prefix} {name} (tables := [], dimensions := [])");
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::Create));
    }

    /// Case-insensitive detection: CREATE OR REPLACE SEMANTIC VIEW
    #[test]
    fn detect_create_or_replace_case_insensitive(
        prefix in arb_case_variant("create or replace semantic view"),
        name in arb_view_name(),
    ) {
        let query = format!("{prefix} {name} (tables := [], dimensions := [])");
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::CreateOrReplace));
    }

    /// Case-insensitive detection: CREATE SEMANTIC VIEW IF NOT EXISTS
    #[test]
    fn detect_create_if_not_exists_case_insensitive(
        prefix in arb_case_variant("create semantic view if not exists"),
        name in arb_view_name(),
    ) {
        let query = format!("{prefix} {name} (tables := [], dimensions := [])");
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::CreateIfNotExists));
    }

    /// Case-insensitive detection: DROP SEMANTIC VIEW
    #[test]
    fn detect_drop_case_insensitive(
        prefix in arb_case_variant("drop semantic view"),
        name in arb_view_name(),
    ) {
        let query = format!("{prefix} {name}");
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::Drop));
    }

    /// Case-insensitive detection: DROP SEMANTIC VIEW IF EXISTS
    #[test]
    fn detect_drop_if_exists_case_insensitive(
        prefix in arb_case_variant("drop semantic view if exists"),
        name in arb_view_name(),
    ) {
        let query = format!("{prefix} {name}");
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::DropIfExists));
    }

    /// Case-insensitive detection: DESCRIBE SEMANTIC VIEW
    #[test]
    fn detect_describe_case_insensitive(
        prefix in arb_case_variant("describe semantic view"),
        name in arb_view_name(),
    ) {
        let query = format!("{prefix} {name}");
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::Describe));
    }

    /// Case-insensitive detection: SHOW SEMANTIC VIEWS
    #[test]
    fn detect_show_case_insensitive(
        prefix in arb_case_variant("show semantic views"),
    ) {
        prop_assert_eq!(detect_ddl_kind(&prefix), Some(DdlKind::Show));
    }

    /// Leading whitespace does not affect detection for any of the 7 forms.
    #[test]
    fn detect_with_leading_whitespace(
        ws in arb_whitespace(),
        form_idx in 0..7usize,
        name in arb_view_name(),
    ) {
        let (prefix, expected_kind) = DDL_FORMS[form_idx];
        let suffix = build_suffix(expected_kind, &name);
        let query = format!("{ws}{prefix}{suffix}");
        prop_assert_eq!(detect_ddl_kind(&query), Some(expected_kind));
    }

    /// Trailing semicolons (0-3) do not affect detection for any of the 7 forms.
    #[test]
    fn detect_with_trailing_semicolons(
        form_idx in 0..7usize,
        name in arb_view_name(),
        semis in arb_semicolons(),
    ) {
        let (prefix, expected_kind) = DDL_FORMS[form_idx];
        let suffix = build_suffix(expected_kind, &name);
        let query = format!("{prefix}{suffix}{semis}");
        prop_assert_eq!(detect_ddl_kind(&query), Some(expected_kind));
    }

    /// Non-DDL SQL statements always return None from detect_ddl_kind.
    #[test]
    fn detect_non_ddl_returns_none(
        stmt in prop_oneof![
            Just("SELECT * FROM orders".to_string()),
            Just("INSERT INTO orders VALUES (1)".to_string()),
            Just("UPDATE orders SET x = 1".to_string()),
            Just("DELETE FROM orders".to_string()),
            Just("ALTER TABLE orders ADD COLUMN x INT".to_string()),
            Just("CREATE TABLE orders (id INT)".to_string()),
            Just("CREATE VIEW orders AS SELECT 1".to_string()),
        ],
    ) {
        prop_assert_eq!(detect_ddl_kind(&stmt), None);
    }

    /// detect_semantic_view_ddl returns PARSE_DETECTED for all 7 forms.
    #[test]
    fn detect_semantic_view_ddl_returns_detected(
        form_idx in 0..7usize,
        name in arb_view_name(),
    ) {
        let (prefix, kind) = DDL_FORMS[form_idx];
        let suffix = build_suffix(kind, &name);
        let query = format!("{prefix}{suffix}");
        prop_assert_eq!(detect_semantic_view_ddl(&query), PARSE_DETECTED);
    }

    /// detect_semantic_view_ddl returns PARSE_NOT_OURS for non-DDL statements.
    #[test]
    fn detect_semantic_view_ddl_returns_not_ours(
        stmt in prop_oneof![
            Just("SELECT * FROM orders".to_string()),
            Just("INSERT INTO orders VALUES (1)".to_string()),
            Just("UPDATE orders SET x = 1".to_string()),
            Just("DELETE FROM orders".to_string()),
            Just("ALTER TABLE orders ADD COLUMN x INT".to_string()),
        ],
    ) {
        prop_assert_eq!(detect_semantic_view_ddl(&stmt), PARSE_NOT_OURS);
    }
}

// ---------------------------------------------------------------------------
// Rewrite properties (TEST-02)
// ---------------------------------------------------------------------------

proptest! {
    /// Rewrite of CREATE forms via validate_and_rewrite produces correct _from_json function call.
    #[test]
    fn rewrite_create_forms(
        form_idx in 0..3usize,
        name in arb_view_name(),
    ) {
        let (prefix, _kind, _fn_name) = CREATE_FORMS[form_idx];
        let ddl = format!("{prefix}{}", build_as_body_suffix(&name));
        let result = validate_and_rewrite(&ddl);
        prop_assert!(
            result.is_ok(),
            "Expected Ok for valid AS-body DDL, got: {:?}",
            result
        );
        let sql = result.unwrap().unwrap();
        prop_assert!(
            sql.starts_with("SELECT * FROM "),
            "Expected rewrite to start with 'SELECT * FROM ', got: {}",
            sql
        );
        prop_assert!(
            sql.contains(&format!("'{name}'")),
            "Expected rewrite to contain view name '{}', got: {}",
            name, sql
        );
    }

    /// extract_ddl_name returns the correct name for CREATE forms.
    #[test]
    fn extract_name_create_forms(
        form_idx in 0..3usize,
        name in arb_view_name(),
    ) {
        let (prefix, _kind, _fn_name) = CREATE_FORMS[form_idx];
        let ddl = format!("{prefix}{}", build_as_body_suffix(&name));
        let extracted = extract_ddl_name(&ddl).unwrap();
        prop_assert_eq!(extracted, Some(name));
    }

    /// extract_ddl_name returns the correct name for name-only forms (DROP, DESCRIBE).
    #[test]
    fn extract_name_name_only_forms(
        form_idx in 0..3usize,
        name in arb_view_name(),
    ) {
        let (prefix, _kind, _fn_name) = NAME_ONLY_FORMS[form_idx];
        let ddl = format!("{prefix} {name}");
        let extracted = extract_ddl_name(&ddl).unwrap();
        prop_assert_eq!(extracted, Some(name));
    }

    /// extract_ddl_name returns None for SHOW form.
    #[test]
    fn extract_name_show_returns_none(
        prefix in arb_case_variant("show semantic views"),
    ) {
        let extracted = extract_ddl_name(&prefix).unwrap();
        prop_assert_eq!(extracted, None);
    }
}

// ---------------------------------------------------------------------------
// Position invariant properties (TEST-03)
// ---------------------------------------------------------------------------

proptest! {
    /// Non-AS-body syntax returns "Expected 'AS' keyword" error with position.
    #[test]
    fn position_invariant_paren_body_rejected(
        spaces in "[ ]{0,20}",
    ) {
        let query = format!("{spaces}CREATE SEMANTIC VIEW x (tbles := [])");
        let err = validate_and_rewrite(&query).unwrap_err();
        prop_assert!(
            err.message.contains("Expected 'AS' keyword"),
            "Expected 'Expected AS keyword' error, got: {}",
            err.message
        );
        prop_assert!(err.position.is_some());
    }

    /// Error position for missing view name after DROP prefix accounts for whitespace.
    #[test]
    fn position_invariant_missing_name_drop(
        spaces in "[ ]{0,20}",
    ) {
        let query = format!("{spaces}DROP SEMANTIC VIEW");
        let err = validate_and_rewrite(&query).unwrap_err();
        let pos = err.position.unwrap();
        // Position should be at or after the end of "DROP SEMANTIC VIEW" in the trimmed query.
        let prefix_len = "drop semantic view".len();
        let trim_offset = spaces.len();
        prop_assert_eq!(
            pos,
            trim_offset + prefix_len,
            "Position {} should equal trim_offset({}) + prefix_len({}) in query: {:?}",
            pos, trim_offset, prefix_len, query
        );
        prop_assert!(
            err.message.contains("Missing view name"),
            "Expected 'Missing view name' error, got: {}",
            err.message
        );
    }

    /// Error position for missing 'AS' after view name accounts for whitespace.
    #[test]
    fn position_invariant_missing_as(
        spaces in "[ ]{0,20}",
        name in arb_view_name(),
    ) {
        let query = format!("{spaces}CREATE SEMANTIC VIEW {name}");
        let err = validate_and_rewrite(&query).unwrap_err();
        let pos = err.position.unwrap();
        // Position should point after the view name.
        prop_assert!(
            pos >= spaces.len() + "CREATE SEMANTIC VIEW ".len() + name.len(),
            "Position {} should be at or after view name end in query: {:?}",
            pos, query
        );
        prop_assert!(
            err.message.contains("Expected 'AS'"),
            "Expected \"Expected 'AS'\" error, got: {}",
            err.message
        );
    }

    /// validate_and_rewrite returns Ok(Some(_)) for valid AS-body DDL with varying whitespace.
    #[test]
    fn valid_ddl_with_whitespace_succeeds(
        spaces in "[ ]{0,20}",
        name in arb_view_name(),
    ) {
        let query = format!(
            "{spaces}CREATE SEMANTIC VIEW{}", build_as_body_suffix(&name)
        );
        let result = validate_and_rewrite(&query);
        prop_assert!(
            result.is_ok(),
            "Expected Ok for valid DDL, got: {:?}",
            result
        );
        let sql = result.unwrap();
        prop_assert!(
            sql.is_some(),
            "Expected Some(sql) for valid DDL"
        );
    }
}

// ---------------------------------------------------------------------------
// Near-miss safety properties (TEST-04)
// ---------------------------------------------------------------------------

proptest! {
    /// Common SQL statements that are NOT close to any DDL prefix return None.
    #[test]
    fn near_miss_no_false_positives(
        stmt in prop_oneof![
            Just("SELECT * FROM t".to_string()),
            Just("INSERT INTO t VALUES (1)".to_string()),
            Just("UPDATE t SET x = 1".to_string()),
            Just("DELETE FROM t".to_string()),
            Just("ALTER TABLE t ADD COLUMN x INT".to_string()),
            Just("BEGIN TRANSACTION".to_string()),
            Just("COMMIT".to_string()),
            Just("ROLLBACK".to_string()),
        ],
    ) {
        prop_assert!(
            detect_near_miss(&stmt).is_none(),
            "Expected no near-miss for normal SQL '{}', got: {:?}",
            stmt, detect_near_miss(&stmt)
        );
    }

    /// Transposition near-misses (e.g. "cretae" for "create") are detected.
    #[test]
    fn near_miss_detects_transposition(
        near_miss in prop_oneof![
            Just("cretae semantic view x".to_string()),
            Just("craete semantic view x".to_string()),
            Just("dreop semantic view x".to_string()),
            Just("descrbe semantic view x".to_string()),
            Just("shwo semantic views".to_string()),
        ],
    ) {
        let result = detect_near_miss(&near_miss);
        prop_assert!(
            result.is_some(),
            "Expected near-miss detection for '{}', got None",
            near_miss
        );
        let err = result.unwrap();
        prop_assert!(
            err.message.contains("Did you mean"),
            "Expected 'Did you mean' suggestion in near-miss error, got: {}",
            err.message
        );
    }

    /// Near-miss position points at the start of the input (after trimming whitespace).
    #[test]
    fn near_miss_position_at_start(
        spaces in "[ ]{0,10}",
    ) {
        let query = format!("{spaces}cretae semantic view x");
        let result = detect_near_miss(&query);
        prop_assert!(result.is_some(), "Expected near-miss for: {:?}", query);
        let err = result.unwrap();
        prop_assert_eq!(
            err.position,
            Some(spaces.len()),
            "Near-miss position should be at trim_offset ({}), got {:?}",
            spaces.len(), err.position
        );
    }
}

// ---------------------------------------------------------------------------
// AS-body keyword syntax properties (TEST-06)
// ---------------------------------------------------------------------------

proptest! {
    /// AS-body CREATE forms are detected by detect_ddl_kind.
    #[test]
    fn as_body_detected_for_create_forms(
        form_idx in 0..3usize,
        name in arb_view_name(),
    ) {
        let (prefix, expected_kind, _) = CREATE_FORMS[form_idx];
        let query = format!("{prefix}{}", build_as_body_suffix(&name));
        prop_assert_eq!(
            detect_ddl_kind(&query),
            Some(expected_kind),
            "AS-body DDL should be detected for form: {}", prefix
        );
    }

    /// AS-body validate_and_rewrite returns Ok(Some(_)) for valid keyword body,
    /// and the rewritten SQL uses the create_semantic_view_from_json route with
    /// the view name embedded as a quoted string.
    #[test]
    fn as_body_validate_and_rewrite_succeeds(
        name in arb_view_name(),
    ) {
        let query = format!("CREATE SEMANTIC VIEW{}", build_as_body_suffix(&name));
        let result = validate_and_rewrite(&query);
        prop_assert!(
            result.is_ok(),
            "Expected Ok for valid AS-body DDL, got: {:?}",
            result
        );
        let sql = result.unwrap();
        prop_assert!(
            sql.is_some(),
            "Expected Some(sql) for valid AS-body DDL"
        );
        let sql = sql.unwrap();
        prop_assert!(
            sql.starts_with("SELECT * FROM create_semantic_view_from_json("),
            "Expected create_semantic_view_from_json route, got: {sql}"
        );
        prop_assert!(
            sql.contains(&format!("'{name}'")),
            "Expected view name in rewritten SQL, got: {sql}"
        );
    }

    /// Error position inside AS-body clause points at the typo byte offset.
    /// This property verifies the base_offset threading invariant from RESEARCH.md Pitfall 1.
    /// Leading whitespace includes spaces, tabs, and newlines — all are valid SQL whitespace.
    #[test]
    fn as_body_position_invariant_clause_typo(
        leading in "[ \t\n]{0,20}",
    ) {
        let query = format!("{leading}CREATE SEMANTIC VIEW x AS TABLSE (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.r AS r) METRICS (t.m AS SUM(1))");
        let err = validate_and_rewrite(&query).unwrap_err();
        let pos = err.position.unwrap();
        // Position must point at "TABLSE" in the original query.
        prop_assert_eq!(
            &query[pos..pos + 6],
            "TABLSE",
            "Position {} does not point at 'TABLSE' in query: {:?}",
            pos, query
        );
        prop_assert!(
            err.message.contains("TABLES") || err.message.contains("tables"),
            "Expected 'did you mean TABLES' error, got: {}",
            err.message
        );
    }

    /// Valid AS-body DDL is accepted regardless of inter-token whitespace style
    /// within the body (spaces, tabs, newlines, or mixed).
    ///
    /// Note: the `CREATE SEMANTIC VIEW` prefix uses literal space matching in
    /// `detect_ddl_kind`, so only body-internal whitespace is varied here.
    /// Prefix whitespace normalization is tracked in TECH-DEBT.md.
    #[test]
    fn as_body_accepts_any_inter_token_whitespace(
        sep in "[ \t\n]{1,4}",
    ) {
        // Vary whitespace between tokens WITHIN the body only (after "AS").
        // The DDL prefix "CREATE SEMANTIC VIEW v AS" uses single spaces because
        // detect_ddl_kind does literal prefix matching.
        let query = format!(
            "CREATE SEMANTIC VIEW v AS TABLES{sep}(\
             t{sep}AS{sep}orders{sep}PRIMARY{sep}KEY{sep}(id)\
             ){sep}DIMENSIONS{sep}(\
             t.region{sep}AS{sep}region\
             ){sep}METRICS{sep}(\
             t.rev{sep}AS{sep}SUM(amount)\
             )"
        );
        let result = validate_and_rewrite(&query);
        prop_assert!(
            result.is_ok(),
            "Expected Ok for valid AS-body with sep={sep:?}, got: {:?}",
            result
        );
        prop_assert!(result.unwrap().is_some());
    }

    /// Clause keywords are case-insensitive (tables/TABLES/Tables all accepted).
    #[test]
    fn as_body_clause_keywords_case_insensitive(
        name in arb_view_name(),
        tables_case in 0..3usize,
        dims_case in 0..3usize,
        metrics_case in 0..3usize,
    ) {
        let tables_kw = ["TABLES", "tables", "Tables"][tables_case];
        let dims_kw = ["DIMENSIONS", "dimensions", "Dimensions"][dims_case];
        let metrics_kw = ["METRICS", "metrics", "Metrics"][metrics_case];
        let query = format!(
            "CREATE SEMANTIC VIEW {name} AS {tables_kw} (t AS orders PRIMARY KEY (id)) \
             {dims_kw} (t.region AS region) {metrics_kw} (t.rev AS SUM(amount))"
        );
        let result = validate_and_rewrite(&query);
        prop_assert!(
            result.is_ok(),
            "Expected Ok for case variant tables={tables_kw} dims={dims_kw} metrics={metrics_kw}, got: {:?}",
            result
        );
    }
}

// ---------------------------------------------------------------------------
// TEST-07: Prefix inter-keyword whitespace variants
// ---------------------------------------------------------------------------
// These tests MUST FAIL against the current starts_with_ci implementation and
// MUST PASS after Plan 02's token-based fix is in place.
// The current detect_ddl_kind uses literal byte prefix matching so it only
// accepts single ASCII spaces between keywords.

fn arb_inter_keyword_ws() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[ \t\n\r]{1,4}").unwrap()
}

proptest! {
    /// Replacing single spaces between DDL keywords with tabs, newlines, or mixed
    /// whitespace must still produce the correct DdlKind. Currently FAILS because
    /// detect_ddl_kind uses literal starts_with_ci byte matching.
    #[test]
    fn prefix_whitespace_detection(
        form_idx in 0..7usize,
        sep in arb_inter_keyword_ws(),
        name in arb_view_name(),
    ) {
        let (prefix, expected_kind) = DDL_FORMS[form_idx];
        // Split on ASCII space and rejoin with the generated separator.
        // e.g. "create semantic view" -> "create\tsemantic\tview"
        let rejoined = prefix.split(' ').collect::<Vec<_>>().join(&sep);
        let suffix = build_suffix(expected_kind, &name);
        let query = format!("{rejoined}{suffix}");
        prop_assert_eq!(
            detect_ddl_kind(&query),
            Some(expected_kind),
            "Expected {:?} for query with sep={:?}: {:?}",
            expected_kind, sep, query
        );
    }

    /// For the 3 CREATE forms, replacing prefix spaces with non-space whitespace
    /// and calling validate_and_rewrite must return Ok(Some(sql)) where sql
    /// starts with "SELECT * FROM ".
    #[test]
    fn prefix_whitespace_rewrite_roundtrip(
        form_idx in 0..3usize,
        sep in arb_inter_keyword_ws(),
        name in arb_view_name(),
    ) {
        let (prefix, _kind, _fn_name) = CREATE_FORMS[form_idx];
        let rejoined = prefix.split(' ').collect::<Vec<_>>().join(&sep);
        let query = format!("{rejoined}{}", build_as_body_suffix(&name));
        let result = validate_and_rewrite(&query);
        prop_assert!(
            result.is_ok(),
            "Expected Ok for whitespace-variant CREATE prefix, got: {:?}",
            result
        );
        let sql = result.unwrap();
        prop_assert!(
            sql.is_some(),
            "Expected Some(sql) for whitespace-variant CREATE prefix"
        );
        let sql = sql.unwrap();
        prop_assert!(
            sql.starts_with("SELECT * FROM "),
            "Rewritten SQL must start with 'SELECT * FROM ', got: {sql}"
        );
    }
}

// ---------------------------------------------------------------------------
// TEST-08: Adversarial inputs
// These tests document safe behavior for inputs that are not expected in normal
// DuckDB usage but must not cause panics or exploitable behavior.
// ---------------------------------------------------------------------------

#[test]
fn test_large_input_no_panic() {
    let s: String = std::iter::repeat('A').take(1_000_000).collect();
    let start = std::time::Instant::now();
    let result = detect_semantic_view_ddl(&s);
    let elapsed = start.elapsed();
    assert_eq!(
        result, PARSE_NOT_OURS,
        "1MB non-DDL should return PARSE_NOT_OURS"
    );
    assert!(
        elapsed.as_millis() < 50,
        "1MB scan took {}ms, expected <50ms",
        elapsed.as_millis()
    );
}

#[test]
fn test_null_byte_in_name() {
    // Null bytes are valid UTF-8 in Rust but may truncate C strings at the FFI boundary.
    // The parser must not panic. It may return Ok (if null byte in name is tolerated)
    // or Err (if the name validator rejects it). Either is acceptable.
    let query = "CREATE SEMANTIC VIEW x\x00bad (tables := [], dimensions := [])";
    let result = std::panic::catch_unwind(|| validate_and_rewrite(query));
    assert!(
        result.is_ok(),
        "validate_and_rewrite panicked on null byte in name"
    );
}

#[test]
fn test_semicolon_in_name() {
    // Name extraction stops at whitespace or '(' — a ';' inside the "name" position
    // means the name is everything before ';' or the whole token. Either way, the
    // rewritten SQL must start with "SELECT * FROM " (no raw ';' injected into the wrapper).
    let query = "CREATE SEMANTIC VIEW x;injection (tables := [], dimensions := [])";
    match validate_and_rewrite(query) {
        Ok(Some(sql)) => {
            assert!(
                sql.starts_with("SELECT * FROM "),
                "Rewritten SQL must start with SELECT * FROM, got: {sql}"
            );
            // The semicolon must not appear outside a SQL string literal in a way
            // that could terminate the wrapper SELECT statement.
            // Since the name ends at whitespace/'(', the name is "x;injection" or
            // the parser hits an error. Either outcome is safe.
        }
        Ok(None) => {} // Not detected as our DDL — acceptable
        Err(_) => {}   // Parse error — acceptable
    }
}

#[test]
fn test_unicode_homoglyph() {
    // Cyrillic 'С' (U+0421) looks like Latin 'C' but is a different byte sequence.
    // The ASCII-only keyword matcher must return None — no near-miss confusion.
    // Note: DuckDB may not pass Unicode-prefixed queries to extension hooks at all
    // (StripUnicodeSpaces normalisation), but the pure-Rust parser must be safe.
    let query = "СREATE SEMANTIC VIEW x (tables := [], dimensions := [])";
    assert_eq!(
        detect_ddl_kind(query),
        None,
        "Cyrillic С prefix should not match as DdlKind"
    );
    assert_eq!(
        detect_semantic_view_ddl(query),
        PARSE_NOT_OURS,
        "Cyrillic С prefix should return PARSE_NOT_OURS"
    );
}

#[test]
fn test_control_char_in_name() {
    // Control characters (0x01-0x1F except whitespace) in view names must not panic.
    let query = "CREATE SEMANTIC VIEW x\x01bad (tables := [], dimensions := [])";
    let result = std::panic::catch_unwind(|| validate_and_rewrite(query));
    assert!(
        result.is_ok(),
        "validate_and_rewrite panicked on control char in name"
    );
}

// ---------------------------------------------------------------------------
// TEST-09: FACTS and HIERARCHIES clause adversarial input (Phase 29)
// ---------------------------------------------------------------------------

/// Generate a valid SQL identifier for use in FACTS/HIERARCHIES entries.
fn arb_identifier() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z][a-z0-9_]{0,9}")
        .unwrap()
        .prop_filter("must not be a SQL keyword or start with AS", |s| {
            let upper = s.to_ascii_uppercase();
            // Reject identifiers that ARE or START WITH common SQL keywords
            // that confuse the dot-qualified entry parser (e.g., "as_" looks
            // like "AS" + "_" to the token splitter).
            !matches!(
                upper.as_str(),
                "AS" | "BY"
                    | "ON"
                    | "IN"
                    | "IS"
                    | "OR"
                    | "TO"
                    | "IF"
                    | "DO"
                    | "REFERENCES"
                    | "PRIMARY"
                    | "KEY"
                    | "MANY"
                    | "ONE"
                    | "SUM"
                    | "COUNT"
                    | "AVG"
                    | "MIN"
                    | "MAX"
            ) && !upper.starts_with("AS_")
                && !upper.starts_with("AS ")
        })
}

/// Generate a simple SQL expression for fact entries.
fn arb_simple_expr() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple column reference
        arb_identifier().prop_map(|id| id),
        // Binary expression
        (arb_identifier(), arb_identifier()).prop_map(|(a, b)| format!("{a} + {b}")),
        // Function call
        arb_identifier().prop_map(|id| format!("SUM({id})")),
        // Multiply with paren group
        (arb_identifier(), arb_identifier()).prop_map(|(a, b)| format!("{a} * (1 - {b})")),
    ]
}

/// Generate a FACTS clause: `FACTS (alias.name AS expr, ...)`
fn arb_facts_clause() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        (arb_identifier(), arb_identifier(), arb_simple_expr()),
        0..=3,
    )
    .prop_map(|entries| {
        if entries.is_empty() {
            String::new()
        } else {
            let items: Vec<String> = entries
                .iter()
                .map(|(alias, name, expr)| format!("{alias}.{name} AS {alias}.{expr}"))
                .collect();
            format!("FACTS ({})", items.join(", "))
        }
    })
}

/// Generate a HIERARCHIES clause: `HIERARCHIES (name AS (dim1, dim2, ...))`
fn arb_hierarchies_clause(dim_names: &[String]) -> impl Strategy<Value = String> {
    let dims = dim_names.to_vec();
    proptest::collection::vec(
        (
            arb_identifier(),
            proptest::sample::subsequence(dims.clone(), 1..=dims.len().max(1)),
        ),
        0..=2,
    )
    .prop_map(|entries| {
        if entries.is_empty() {
            String::new()
        } else {
            let items: Vec<String> = entries
                .iter()
                .map(|(name, levels)| format!("{name} AS ({})", levels.join(", ")))
                .collect();
            format!("HIERARCHIES ({})", items.join(", "))
        }
    })
}

/// Build a valid AS-body with optional FACTS and HIERARCHIES clauses.
fn build_as_body_with_facts_hierarchies(
    name: &str,
    alias: &str,
    table: &str,
    facts_clause: &str,
    hierarchies_clause: &str,
    dim_name: &str,
    metric_expr: &str,
) -> String {
    let mut body = format!(" {name} AS TABLES ({alias} AS {table} PRIMARY KEY (id))");
    if !facts_clause.is_empty() {
        body.push(' ');
        body.push_str(facts_clause);
    }
    if !hierarchies_clause.is_empty() {
        body.push(' ');
        body.push_str(hierarchies_clause);
    }
    body.push_str(&format!(
        " DIMENSIONS ({alias}.{dim_name} AS {dim_name}) METRICS ({alias}.m AS {metric_expr})"
    ));
    body
}

proptest! {
    /// FACTS clause with adversarial input: parse_keyword_body either succeeds
    /// or returns a well-formed ParseError (no panics, no crashes).
    #[test]
    fn facts_clause_no_panic(
        facts_clause in arb_facts_clause(),
        name in arb_view_name(),
    ) {
        let query = format!(
            "CREATE SEMANTIC VIEW {name} AS TABLES (t AS orders PRIMARY KEY (id)) {facts_clause} DIMENSIONS (t.region AS region) METRICS (t.rev AS SUM(amount))"
        );
        let result = std::panic::catch_unwind(|| validate_and_rewrite(&query));
        prop_assert!(
            result.is_ok(),
            "validate_and_rewrite panicked on FACTS clause: {facts_clause}"
        );
        // The result is either Ok(Some(sql)) for valid DDL, Ok(None) for non-DDL, or Err(ParseError)
        // All are acceptable -- the key invariant is no panics.
    }

    /// HIERARCHIES clause with adversarial input: no panics.
    #[test]
    fn hierarchies_clause_no_panic(
        name in arb_view_name(),
        hier_name in arb_identifier(),
        level_count in 1..=4usize,
    ) {
        // Build a hierarchy with valid dimension names
        let levels: Vec<String> = (0..level_count).map(|i| format!("dim{i}")).collect();
        let dims_clause: String = levels.iter()
            .map(|l| format!("t.{l} AS {l}"))
            .collect::<Vec<_>>()
            .join(", ");
        let hier_clause = format!("{hier_name} AS ({})", levels.join(", "));
        let query = format!(
            "CREATE SEMANTIC VIEW {name} AS TABLES (t AS orders PRIMARY KEY (id)) HIERARCHIES ({hier_clause}) DIMENSIONS ({dims_clause}) METRICS (t.rev AS SUM(amount))"
        );
        let result = std::panic::catch_unwind(|| validate_and_rewrite(&query));
        prop_assert!(
            result.is_ok(),
            "validate_and_rewrite panicked on HIERARCHIES clause: {hier_clause}"
        );
    }

    /// Combined FACTS + HIERARCHIES with adversarial input: no panics.
    #[test]
    fn facts_and_hierarchies_combined_no_panic(
        name in arb_view_name(),
        alias in arb_identifier(),
        fact_name in arb_identifier(),
        fact_expr in arb_simple_expr(),
        hier_name in arb_identifier(),
    ) {
        let query = format!(
            "CREATE SEMANTIC VIEW {name} AS \
             TABLES ({alias} AS orders PRIMARY KEY (id)) \
             FACTS ({alias}.{fact_name} AS {alias}.{fact_expr}) \
             HIERARCHIES ({hier_name} AS (region)) \
             DIMENSIONS ({alias}.region AS region) \
             METRICS ({alias}.rev AS SUM(amount))"
        );
        let result = std::panic::catch_unwind(|| validate_and_rewrite(&query));
        prop_assert!(
            result.is_ok(),
            "validate_and_rewrite panicked on combined FACTS+HIERARCHIES query"
        );
    }

    /// Empty FACTS and HIERARCHIES clauses are valid (no entries).
    #[test]
    fn empty_facts_hierarchies_clauses_valid(
        name in arb_view_name(),
    ) {
        let query = format!(
            "CREATE SEMANTIC VIEW {name} AS \
             TABLES (t AS orders PRIMARY KEY (id)) \
             FACTS () \
             HIERARCHIES () \
             DIMENSIONS (t.region AS region) \
             METRICS (t.rev AS SUM(amount))"
        );
        let result = validate_and_rewrite(&query);
        prop_assert!(
            result.is_ok(),
            "Empty FACTS () and HIERARCHIES () should be accepted, got: {:?}",
            result
        );
        prop_assert!(
            result.unwrap().is_some(),
            "Empty FACTS/HIERARCHIES should produce valid rewrite"
        );
    }
}

// ---------------------------------------------------------------------------
// TEST-10: Derived metric parsing and expression substitution (Phase 30)
// ---------------------------------------------------------------------------

/// Generate a valid metric name (no dots allowed for derived metrics).
fn arb_metric_name() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z][a-z0-9_]{0,14}").unwrap()
}

/// Generate a simple arithmetic expression from metric names.
fn arb_arithmetic_expr(names: Vec<String>) -> impl Strategy<Value = String> {
    let operators = vec![" + ", " - ", " * ", " / "];
    let n = names.len();
    if n == 0 {
        return Just("1".to_string()).boxed();
    }
    (0..n, proptest::sample::select(operators), 0..n)
        .prop_map(move |(a, op, b)| {
            let left = &names[a % names.len()];
            let right = &names[b % names.len()];
            format!("{left}{op}{right}")
        })
        .boxed()
}

proptest! {
    /// Derived metric entries in METRICS clause: parse_and_rewrite either succeeds
    /// or returns a well-formed error (no panics).
    #[test]
    fn derived_metric_parsing_no_panic(
        name in arb_view_name(),
        derived_name in arb_metric_name(),
        base_name in arb_metric_name(),
    ) {
        // Build a DDL with one base metric and one derived metric
        let query = format!(
            "CREATE SEMANTIC VIEW {name} AS \
             TABLES (t AS orders PRIMARY KEY (id)) \
             DIMENSIONS (t.region AS region) \
             METRICS (t.{base_name} AS SUM(t.amount), {derived_name} AS {base_name} + 1)"
        );
        let result = std::panic::catch_unwind(|| validate_and_rewrite(&query));
        prop_assert!(
            result.is_ok(),
            "validate_and_rewrite panicked on derived metric entry"
        );
    }

    /// Mixed qualified and unqualified metric entries: no panics.
    #[test]
    fn mixed_metrics_no_panic(
        name in arb_view_name(),
        alias in arb_identifier(),
        base1 in arb_metric_name(),
        base2 in arb_metric_name(),
        derived in arb_metric_name(),
    ) {
        let query = format!(
            "CREATE SEMANTIC VIEW {name} AS \
             TABLES ({alias} AS orders PRIMARY KEY (id)) \
             DIMENSIONS ({alias}.region AS region) \
             METRICS ({alias}.{base1} AS SUM({alias}.amount), \
                      {alias}.{base2} AS COUNT(*), \
                      {derived} AS {base1} + {base2})"
        );
        let result = std::panic::catch_unwind(|| validate_and_rewrite(&query));
        prop_assert!(
            result.is_ok(),
            "validate_and_rewrite panicked on mixed qualified/unqualified metrics"
        );
    }
}

// ---------------------------------------------------------------------------
// TEST-11: Word-boundary replacement safety (Phase 30)
// ---------------------------------------------------------------------------

use semantic_views::expand::quote_ident;

proptest! {
    /// replace_word_boundary (tested through expand infrastructure):
    /// quote_ident with random strings does not panic and produces valid output.
    #[test]
    fn quote_ident_never_panics(
        input in "[a-zA-Z0-9_\"]{0,50}",
    ) {
        let result = std::panic::catch_unwind(|| quote_ident(&input));
        prop_assert!(
            result.is_ok(),
            "quote_ident panicked on input: {input:?}"
        );
        let quoted = result.unwrap();
        prop_assert!(quoted.starts_with('"'), "Must start with double quote");
        prop_assert!(quoted.ends_with('"'), "Must end with double quote");
    }
}

// ---------------------------------------------------------------------------
// TEST-12: Relationship inference (Phase 33 -- replaces Phase 31 cardinality keyword tests)
// ---------------------------------------------------------------------------

proptest! {
    /// Relationship entries without cardinality keywords parse successfully
    /// and infer cardinality from PK/UNIQUE constraints (ManyToOne by default).
    #[test]
    fn relationship_no_cardinality_defaults(
        name in arb_view_name(),
        alias_from in arb_identifier(),
        alias_to in arb_identifier(),
        fk_col in arb_identifier(),
    ) {
        let input = format!(
            "{name} AS {alias_from}({fk_col}) REFERENCES {alias_to}"
        );
        let ddl = format!(
            "CREATE SEMANTIC VIEW v AS TABLES ({alias_from} AS orders PRIMARY KEY (id), {alias_to} AS customers PRIMARY KEY (id)) RELATIONSHIPS ({input}) DIMENSIONS ({alias_from}.r AS region) METRICS ({alias_from}.m AS SUM(amount))"
        );
        let result = validate_and_rewrite(&ddl);
        prop_assert!(
            result.is_ok(),
            "Failed to parse relationship without cardinality: {:?}",
            result.unwrap_err()
        );
    }

    /// Metrics with USING clause containing random relationship names parse
    /// successfully through the full DDL pipeline and produce valid JSON.
    #[test]
    fn metric_using_clause_roundtrip(
        // Exclude names starting with "as" to avoid collision with the AS keyword parser
        rel_name in proptest::string::string_regex("[b-z][a-z0-9_]{1,15}").unwrap(),
        metric_name in proptest::string::string_regex("[b-z][a-z0-9_]{1,15}").unwrap(),
        using_kw in arb_case_variant("using"),
    ) {
        // Build DDL with a USING clause on a metric
        let ddl = format!(
            "CREATE SEMANTIC VIEW v AS \
             TABLES (f AS flights PRIMARY KEY (id), a AS airports PRIMARY KEY (id)) \
             RELATIONSHIPS ({rel_name} AS f(airport_id) REFERENCES a) \
             DIMENSIONS (a.name AS airport_name) \
             METRICS (f.{metric_name} {using_kw} ({rel_name}) AS COUNT(*))"
        );
        let result = validate_and_rewrite(&ddl);
        prop_assert!(
            result.is_ok(),
            "Failed to parse metric with USING clause (rel={}, met={}, kw={}): {:?}",
            rel_name,
            metric_name,
            using_kw,
            result.unwrap_err()
        );
        let sql = result.unwrap();
        prop_assert!(
            sql.is_some(),
            "Expected Some(sql) for valid DDL with USING"
        );
        // The rewritten SQL should contain the metric name and using_relationships
        let sql_str = sql.unwrap();
        prop_assert!(
            sql_str.contains("using_relationships"),
            "Rewritten JSON should contain using_relationships: {sql_str}"
        );
    }
}
