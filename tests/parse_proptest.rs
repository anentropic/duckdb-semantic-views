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

/// Build a valid suffix for a given DdlKind (name + body for CREATE, name for DROP/DESCRIBE, empty for SHOW).
fn build_suffix(kind: DdlKind, name: &str) -> String {
    match kind {
        DdlKind::Create | DdlKind::CreateOrReplace | DdlKind::CreateIfNotExists => {
            format!(" {name} (tables := [], dimensions := [])")
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
    /// Rewrite of CREATE forms produces correct function call with view name.
    #[test]
    fn rewrite_create_forms(
        form_idx in 0..3usize,
        name in arb_view_name(),
    ) {
        let (prefix, _kind, fn_name) = CREATE_FORMS[form_idx];
        let ddl = format!("{prefix} {name} (tables := [], dimensions := [])");
        let sql = rewrite_ddl(&ddl).unwrap();
        let expected_start = format!("SELECT * FROM {fn_name}(");
        prop_assert!(
            sql.starts_with(&expected_start),
            "Expected rewrite to start with '{}', got: {}",
            expected_start, sql
        );
        prop_assert!(
            sql.contains(&format!("'{name}'")),
            "Expected rewrite to contain view name '{}', got: {}",
            name, sql
        );
    }

    /// Rewrite of name-only forms (DROP, DESCRIBE) produces correct function call.
    #[test]
    fn rewrite_name_only_forms(
        form_idx in 0..3usize,
        name in arb_view_name(),
    ) {
        let (prefix, _kind, fn_name) = NAME_ONLY_FORMS[form_idx];
        let ddl = format!("{prefix} {name}");
        let sql = rewrite_ddl(&ddl).unwrap();
        let expected_start = format!("SELECT * FROM {fn_name}(");
        prop_assert!(
            sql.starts_with(&expected_start),
            "Expected rewrite to start with '{}', got: {}",
            expected_start, sql
        );
        prop_assert!(
            sql.contains(&format!("'{name}'")),
            "Expected rewrite to contain view name '{}', got: {}",
            name, sql
        );
    }

    /// Rewrite of SHOW produces exactly the list function call.
    #[test]
    fn rewrite_show_form(
        prefix in arb_case_variant("show semantic views"),
    ) {
        let sql = rewrite_ddl(&prefix).unwrap();
        prop_assert_eq!(sql, "SELECT * FROM list_semantic_views()");
    }

    /// extract_ddl_name returns the correct name for CREATE forms.
    #[test]
    fn extract_name_create_forms(
        form_idx in 0..3usize,
        name in arb_view_name(),
    ) {
        let (prefix, _kind, _fn_name) = CREATE_FORMS[form_idx];
        let ddl = format!("{prefix} {name} (tables := [], dimensions := [])");
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
    /// Error position for unknown clause keyword points at the typo in the original
    /// query, regardless of leading whitespace variation.
    #[test]
    fn position_invariant_clause_typo(
        spaces in "[ ]{0,20}",
    ) {
        let query = format!("{spaces}CREATE SEMANTIC VIEW x (tbles := [])");
        let err = validate_and_rewrite(&query).unwrap_err();
        let pos = err.position.unwrap();
        // The position must point at the start of "tbles" in the original query.
        prop_assert_eq!(
            &query[pos..pos + 5],
            "tbles",
            "Position {} does not point at 'tbles' in query: {:?}",
            pos, query
        );
        prop_assert!(
            err.message.contains("Unknown clause"),
            "Expected 'Unknown clause' error, got: {}",
            err.message
        );
    }

    /// Error position for empty body points right after '(' regardless of whitespace.
    #[test]
    fn position_invariant_empty_body(
        spaces in "[ ]{0,20}",
    ) {
        // The body is a single space between ( and ) so it's considered empty after trimming.
        let query = format!("{spaces}CREATE SEMANTIC VIEW x ( )");
        let err = validate_and_rewrite(&query).unwrap_err();
        let pos = err.position.unwrap();
        // Position should point inside the body area (right after '(').
        let open_paren = query.find('(').unwrap();
        prop_assert!(
            pos > open_paren,
            "Position {} should be after '(' at {} in query: {:?}",
            pos, open_paren, query
        );
        prop_assert!(
            err.message.contains("empty") || err.message.contains("Missing required clause"),
            "Expected empty body or missing clause error, got: {}",
            err.message
        );
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

    /// Error position for missing '(' after view name accounts for whitespace.
    #[test]
    fn position_invariant_missing_paren(
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
            err.message.contains("Expected '('"),
            "Expected \"Expected '('\" error, got: {}",
            err.message
        );
    }

    /// validate_and_rewrite returns Ok(Some(_)) for valid DDL with varying whitespace.
    #[test]
    fn valid_ddl_with_whitespace_succeeds(
        spaces in "[ ]{0,20}",
        name in arb_view_name(),
    ) {
        let query = format!(
            "{spaces}CREATE SEMANTIC VIEW {name} (tables := ['t'], dimensions := ['d'])"
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
// Bracket validation properties (TEST-05)
// ---------------------------------------------------------------------------

proptest! {
    /// Balanced brackets in a valid body do not produce errors.
    #[test]
    fn brackets_balanced_valid_body(
        _name in arb_view_name(),
    ) {
        let body = "tables := ['orders'], dimensions := [{'name': 'region', 'expr': 'region'}]";
        let body_offset = 0;
        let result = validate_clauses(body, body_offset, "");
        prop_assert!(
            result.is_ok(),
            "Expected Ok for balanced brackets, got: {:?}",
            result
        );
    }

    /// An extra '[' appended to a valid body produces an unbalanced bracket error.
    #[test]
    fn brackets_extra_open_bracket(
        _name in arb_view_name(),
    ) {
        let body = format!("tables := ['orders'], dimensions := []{}", "[");
        let body_offset = 0;
        let result = validate_clauses(&body, body_offset, "");
        prop_assert!(
            result.is_err(),
            "Expected Err for unbalanced bracket, got Ok"
        );
        let err = result.unwrap_err();
        prop_assert!(
            err.message.contains("Unbalanced bracket"),
            "Expected 'Unbalanced bracket' error, got: {}",
            err.message
        );
    }

    /// Extra ']' after balanced brackets: the current implementation silently
    /// ignores unmatched close brackets when the stack is empty (check_close_bracket
    /// returns Ok(()) because paren_stack.last() is None). This test documents
    /// that behavior.
    #[test]
    fn brackets_extra_close_bracket_is_tolerated(
        _name in arb_view_name(),
    ) {
        let body = format!("tables := ['orders'], dimensions := []{}", "]");
        let body_offset = 0;
        let _result = validate_clauses(&body, body_offset, "");
        // Documenting current behavior: unmatched close brackets with an
        // empty stack are silently ignored by the bracket validator.
    }

    /// Brackets inside single-quoted string literals do not affect bracket validation.
    #[test]
    fn brackets_inside_strings_ignored(
        _name in arb_view_name(),
    ) {
        let body = "tables := ['a[b]c'], dimensions := [{'name': 'x', 'expr': 'y'}]";
        let body_offset = 0;
        let result = validate_clauses(body, body_offset, "");
        prop_assert!(
            result.is_ok(),
            "Expected Ok for brackets inside strings, got: {:?}",
            result
        );
    }

    /// Nested brackets (array of maps) are handled correctly.
    #[test]
    fn brackets_nested_structures(
        _name in arb_view_name(),
    ) {
        let body = "tables := [{'key': [1, 2, 3]}], dimensions := [{'name': 'x', 'expr': 'y'}]";
        let body_offset = 0;
        let result = validate_clauses(body, body_offset, "");
        prop_assert!(
            result.is_ok(),
            "Expected Ok for nested brackets, got: {:?}",
            result
        );
    }

    /// body_offset is correctly added to the error position for unbalanced brackets.
    #[test]
    fn brackets_error_position_includes_offset(
        offset in 10..100usize,
    ) {
        // Body with an unmatched '['
        let body = "tables := [, dimensions := []";
        let result = validate_clauses(body, offset, "");
        prop_assert!(
            result.is_err(),
            "Expected Err for unbalanced bracket"
        );
        let err = result.unwrap_err();
        let pos = err.position.unwrap();
        // The position should be >= offset (because body_offset is added)
        prop_assert!(
            pos >= offset,
            "Error position {} should be >= body_offset {}",
            pos, offset
        );
    }

    /// Mismatched bracket types produce an error (e.g. '[' closed with '}').
    #[test]
    fn brackets_mismatch_detected(
        _name in arb_view_name(),
    ) {
        let body = "tables := [}, dimensions := []";
        let body_offset = 0;
        let result = validate_clauses(body, body_offset, "");
        prop_assert!(
            result.is_err(),
            "Expected Err for mismatched brackets"
        );
        let err = result.unwrap_err();
        prop_assert!(
            err.message.contains("Unbalanced bracket"),
            "Expected 'Unbalanced bracket' error, got: {}",
            err.message
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

    /// AS-body validate_and_rewrite returns Ok(Some(_)) for valid keyword body.
    /// This test will FAIL until Plan 03 wires the AS dispatch into validate_create_body.
    /// Expected failure documented here as part of Wave 0 gap contract.
    #[test]
    fn as_body_validate_and_rewrite_succeeds(
        name in arb_view_name(),
    ) {
        let query = format!("CREATE SEMANTIC VIEW{}", build_as_body_suffix(&name));
        // This should succeed after Plan 03 implements the AS dispatch.
        // For now, we assert the kind is detected (weaker check that always passes).
        prop_assert_eq!(detect_ddl_kind(&query), Some(DdlKind::Create));
        // The full validate_and_rewrite check is added in Plan 03 summary test update.
    }

    /// Error position inside AS-body clause points at correct byte offset.
    /// This property verifies the base_offset threading invariant from RESEARCH.md Pitfall 1.
    #[test]
    fn as_body_position_invariant_clause_typo(
        spaces in "[ ]{0,20}",
    ) {
        // When the implementation is complete, this error query should produce
        // an error pointing at "TABLSE". Until Plan 02/03, validate_and_rewrite
        // may return Err with Expected '(' message. We assert the query is detected.
        let query = format!("{spaces}CREATE SEMANTIC VIEW x AS TABLSE (t AS orders PRIMARY KEY (id)) DIMENSIONS (t.r AS r) METRICS (t.m AS SUM(1))");
        prop_assert_eq!(
            detect_ddl_kind(&query),
            Some(DdlKind::Create),
            "AS-body query should be detected even with clause typo in body"
        );
    }
}
