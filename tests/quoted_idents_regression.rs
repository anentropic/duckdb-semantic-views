//! Phase 64 regression test — quoted identifier handling.
//!
//! These inputs were the bug-report reproductions for Phase 64.
//! They must never regress: `validate_and_rewrite()` must accept each
//! one and return rewritten SQL where the stored view name is the bare
//! unquoted last part of the identifier
//! (e.g. `"memory"."main"."orders_sv"` → `orders_sv`).
//!
//! Companion seed files for the libfuzzer target:
//!   - fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_bare.txt
//!   - fuzz/seeds/fuzz_ddl_parse/seed_phase64_quoted_fqn.txt
//!   - fuzz/seeds/fuzz_ddl_parse/seed_phase64_mixed_quoting.txt

use semantic_views::parse::validate_and_rewrite;

const QUOTED_BARE_NAME: &str = "CREATE SEMANTIC VIEW \"memory\".\"main\".\"orders_sv\" AS \
                                TABLES (o AS orders PRIMARY KEY (id)) \
                                DIMENSIONS (o.r AS o.r) \
                                METRICS (o.t AS SUM(o.a))";

const QUOTED_FQN: &str = "CREATE OR REPLACE SEMANTIC VIEW \"db\".\"s\".\"v\" AS \
                          TABLES (o AS orders PRIMARY KEY (id)) \
                          DIMENSIONS (o.r AS o.r) \
                          METRICS (o.t AS SUM(o.a))";

const MIXED_QUOTING: &str = "CREATE SEMANTIC VIEW db.\"s\".v AS \
                             TABLES (o AS orders PRIMARY KEY (id)) \
                             DIMENSIONS (o.r AS o.r) \
                             METRICS (o.t AS SUM(o.a))";

#[test]
fn fully_quoted_fqn_normalises_to_bare_name() {
    let sql = validate_and_rewrite(QUOTED_BARE_NAME)
        .expect("parse should not error")
        .expect("rewrite should return Some");
    // The rewritten SQL embeds the bare view name as a SQL string literal
    // (e.g. SELECT * FROM create_semantic_view_from_json('orders_sv', '...')).
    assert!(
        sql.contains("'orders_sv'"),
        "rewritten SQL does not embed bare 'orders_sv': {sql}"
    );
    // It must NOT embed the quoted FQN form anywhere.
    assert!(
        !sql.contains("\"memory\".\"main\".\"orders_sv\""),
        "rewritten SQL still embeds quoted FQN view name: {sql}"
    );
    // Sanity: starts with the parser_override prefix.
    assert!(
        sql.starts_with("SELECT * FROM "),
        "unexpected SQL shape: {sql}"
    );
}

#[test]
fn quoted_fqn_short_parts_normalise_to_bare_name() {
    let sql = validate_and_rewrite(QUOTED_FQN)
        .expect("parse should not error")
        .expect("rewrite should return Some");
    assert!(
        sql.contains("'v'"),
        "expected bare 'v' in rewritten SQL: {sql}"
    );
    assert!(
        !sql.contains("\"db\".\"s\".\"v\""),
        "rewritten SQL still embeds quoted FQN view name: {sql}"
    );
    assert!(
        sql.starts_with("SELECT * FROM "),
        "unexpected SQL shape: {sql}"
    );
}

#[test]
fn mixed_quoting_normalises_to_bare_name() {
    let sql = validate_and_rewrite(MIXED_QUOTING)
        .expect("parse should not error")
        .expect("rewrite should return Some");
    assert!(
        sql.contains("'v'"),
        "expected bare 'v' in rewritten SQL: {sql}"
    );
    // Must not embed the mixed-quoted form as a literal view name.
    assert!(
        !sql.contains("db.\"s\".v"),
        "rewritten SQL still embeds mixed-quoting view name: {sql}"
    );
    assert!(
        sql.starts_with("SELECT * FROM "),
        "unexpected SQL shape: {sql}"
    );
}
