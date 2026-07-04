//! Write-side SQL builders for the `semantic_layer._definitions` catalog table
//! (AR-1).
//!
//! These functions build the pure-SQL existence/collision guards that the
//! `parser_override` DROP/ALTER emitters (`crate::parse::native_sql`) prepend
//! to their DML. They live here, next to the table identity
//! ([`super::DEFINITIONS_TABLE`]) and the canonical "does not exist" wording
//! ([`super::view_not_found_msg`]) they mirror, rather than in the parse layer
//! that consumes them. Callers pass names already SQL-escaped (single quotes
//! doubled); each builder embeds them into a single-quoted literal.
//!
//! All three are compiled unconditionally (they have no FFI dependency) so the
//! guard-wording unit tests below run under `cargo test`; the `allow(dead_code)`
//! covers the bundled-non-test build where only the extension emitters call
//! them.

use super::{DEFINITIONS_SCHEMA, DEFINITIONS_TABLE, DEFINITIONS_TABLE_NAME};

/// Build the existence-guard SELECT for non-IF-EXISTS DROP/ALTER.
///
/// `name_escaped` is the view name with single quotes already SQL-doubled
/// (as produced by `escape_sql_arg` at the `rewrite_to_native_sql` boundary).
///
/// The emitted statement errors with `semantic view '<name>' does not
/// exist` when the row is missing from the catalog table (`DEFINITIONS_TABLE`).
/// Caller appends `;` and the actual DELETE/UPDATE; both run on the
/// caller's connection in the same transaction so the guard's NOT EXISTS
/// check is snapshot-consistent with the DML that follows.
///
/// Phase 65 Plan 06: this guard subsumes both (a) the legacy "view never
/// existed" catalog pre-check (retired with H1 `catalog_conn`) AND (b)
/// the Phase 60 race-guard for "row dropped between pre-check and DML".
/// A single "does not exist" message covers both cases — matches the
/// wording the v0.6.0 sqllogictests pin (`phase20_extended_ddl`,
/// `phase34_1_alter_rename`, `phase45_alter_comment`, `65_alter_*`).
///
/// The CTE form `WITH op AS (DELETE ... RETURNING)` is rejected by `DuckDB`
/// 1.10.502 with `Parser Error: A CTE needs a SELECT`, so we use a
/// two-statement string instead. See the smoke test
/// `catalog::tests::two_statement_guard_then_dml_smoke` for the working shape.
/// Phase 65.1 Plan 04 (WR-03): outer `information_schema` guard.
///
/// Emits a SELECT that errors with the canonical
/// `semantic view '<name>' does not exist` wording when
/// `semantic_layer._definitions` is missing (e.g. a fresh RO DB that was
/// never RW-LOADed, so `init_catalog` never ran). Designed to run as the
/// FIRST statement in a multi-statement string so the subsequent
/// statements (which reference `_definitions` directly) never bind on a
/// never-bootstrapped DB — `DuckDB` binds and executes multi-statement
/// strings one statement at a time, so a failure here short-circuits the
/// rest (empirically verified — see Plan 04 SUMMARY for probe notes).
///
/// We deliberately do NOT collapse this into a single CASE expression
/// with `existence_guard_select`: `DuckDB` binds CASE branches eagerly, so
/// the inner `SELECT 1 FROM semantic_layer._definitions ...` would still
/// fail to bind on missing-table even if the outer WHEN guarantees it
/// would never evaluate at runtime.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn definitions_table_guard_select(name_escaped: &str) -> String {
    format!(
        "SELECT CASE \
              WHEN NOT EXISTS (SELECT 1 FROM information_schema.tables \
                                WHERE table_schema = '{DEFINITIONS_SCHEMA}' \
                                  AND table_name = '{DEFINITIONS_TABLE_NAME}') \
                THEN error('semantic view ''{name_escaped}'' does not exist') \
              ELSE TRUE \
            END"
    )
}

#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn existence_guard_select(name_escaped: &str) -> String {
    format!(
        "SELECT CASE WHEN NOT EXISTS \
                   (SELECT 1 FROM {DEFINITIONS_TABLE} WHERE name = '{name_escaped}') \
                THEN error('semantic view ''{name_escaped}'' does not exist') \
                ELSE TRUE END"
    )
}

/// Build the "target name must NOT already exist" guard for ALTER RENAME.
/// Errors with `semantic view '<new_name>' already exists` if a row with
/// the new name is found in `semantic_layer._definitions`. Runs on the
/// caller's connection in the same transaction as the UPDATE so its
/// EXISTS check is snapshot-consistent with the DML.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn rename_collision_guard_select(new_name_escaped: &str) -> String {
    format!(
        "SELECT CASE WHEN EXISTS \
                   (SELECT 1 FROM {DEFINITIONS_TABLE} WHERE name = '{new_name_escaped}') \
                THEN error('semantic view ''{new_name_escaped}'' already exists') \
                ELSE TRUE END"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existence_guard_select_emits_not_exists_and_error() {
        let g = existence_guard_select("sales");
        assert!(g.contains("NOT EXISTS"), "missing NOT EXISTS: {g}");
        assert!(
            g.contains("FROM semantic_layer._definitions WHERE name = 'sales'"),
            "guard targets wrong table/predicate: {g}"
        );
        assert!(
            g.contains("error('semantic view ''sales'' does not exist')"),
            "missing error() with 'does not exist' wording: {g}"
        );
        // Must be a SELECT (so it can run as the first of two statements
        // without affecting catalog state when the row is present).
        assert!(g.trim_start().starts_with("SELECT "), "not a SELECT: {g}");
        // Must not contain a trailing ';' — the caller appends ';' + DML.
        assert!(!g.contains(';'), "guard must not include ';' itself: {g}");
    }

    #[test]
    fn definitions_table_guard_emits_information_schema_check() {
        // Phase 65.1 Plan 04 (WR-03): the table-guard SELECT runs as the
        // FIRST statement of the DROP/ALTER rewrite. It checks
        // information_schema for `_definitions` and errors with the
        // canonical "does not exist" wording when the table is missing.
        // It does NOT touch `_definitions` itself — bind-time-safe on a
        // never-bootstrapped RO DB.
        let g = definitions_table_guard_select("sales");
        assert!(
            g.contains("information_schema.tables"),
            "missing information_schema guard: {g}"
        );
        assert!(
            g.contains("table_schema = 'semantic_layer'"),
            "guard missing schema predicate: {g}"
        );
        assert!(
            g.contains("table_name = '_definitions'"),
            "guard missing table predicate: {g}"
        );
        assert!(
            g.contains("error('semantic view ''sales'' does not exist')"),
            "missing canonical wording: {g}"
        );
        // Must NOT touch `semantic_layer._definitions` directly — that's
        // the whole point of running this BEFORE the row guard / DML.
        assert!(
            !g.contains("FROM semantic_layer._definitions"),
            "table guard must not bind against _definitions (defeats the purpose): {g}"
        );
        assert!(g.trim_start().starts_with("SELECT "), "not a SELECT: {g}");
        assert!(!g.contains(';'), "guard must not include ';' itself: {g}");
    }

    #[test]
    fn definitions_table_guard_escapes_quotes_in_name() {
        // Quote-doubling for embedded `'` inside the canonical error
        // wording — same convention as `existence_guard_select`.
        let g = definitions_table_guard_select("O''Brien");
        assert!(
            g.contains("error('semantic view ''O''Brien'' does not exist')"),
            "error message wrong: {g}"
        );
    }

    #[test]
    fn existence_guard_select_doubles_quotes_in_name() {
        // name_escaped already has '' for single quotes; embedding it inside
        // an outer SQL string literal preserves correct decoding (DuckDB
        // sees ''X'' as 'X' in the literal). The user-facing error message
        // must read: semantic view 'O'Brien' does not exist.
        let g = existence_guard_select("O''Brien");
        assert!(
            g.contains("WHERE name = 'O''Brien'"),
            "WHERE clause wrong: {g}"
        );
        assert!(
            g.contains("error('semantic view ''O''Brien'' does not exist')"),
            "error message wrong: {g}"
        );
    }

    #[test]
    fn rename_collision_guard_select_emits_exists_and_error() {
        let g = rename_collision_guard_select("taken");
        assert!(g.contains("EXISTS"), "missing EXISTS: {g}");
        assert!(
            !g.contains("NOT EXISTS"),
            "must be EXISTS, not NOT EXISTS: {g}"
        );
        assert!(
            g.contains("FROM semantic_layer._definitions WHERE name = 'taken'"),
            "guard targets wrong table/predicate: {g}"
        );
        assert!(
            g.contains("error('semantic view ''taken'' already exists')"),
            "missing error() with 'already exists' wording: {g}"
        );
        assert!(g.trim_start().starts_with("SELECT "), "not a SELECT: {g}");
        assert!(!g.contains(';'), "guard must not include ';' itself: {g}");
    }
}
