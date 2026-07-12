//! Write-side SQL builders for the `semantic_layer._definitions` catalog table
//! (AR-1).
//!
//! These functions build the pure-SQL existence/collision guards that the
//! `parser_override` DROP/ALTER emitters (`crate::parse::native_sql`) prepend
//! to their DML. They live here, next to the table identity
//! ([`super::DEFINITIONS_TABLE`]) and the canonical "does not exist" wording
//! ([`super::view_not_found_msg`]) they mirror, rather than in the parse layer
//! that consumes them. Callers pass a [`crate::sql_lit::SqlLit`] (a name
//! already `''`-escaped exactly once); each builder embeds it into a
//! single-quoted literal.
//!
//! All three are compiled unconditionally (they have no FFI dependency) so the
//! guard-wording unit tests below run under `cargo test`; the `allow(dead_code)`
//! covers the bundled-non-test build where only the extension emitters call
//! them.

use super::{DEFINITIONS_SCHEMA, DEFINITIONS_TABLE, DEFINITIONS_TABLE_NAME};
use crate::sql_lit::SqlLit;

/// Build the existence-guard SELECT for non-IF-EXISTS DROP/ALTER.
///
/// `name` is the view name already `''`-escaped as a [`SqlLit`] (produced
/// via `SqlLit::escape` at the `rewrite_to_native_sql` boundary).
///
/// The emitted statement errors with `semantic view '<name>' does not
/// exist` when the row is missing from the catalog table (`DEFINITIONS_TABLE`).
/// Caller appends `;` and the actual DELETE/UPDATE.
///
/// # Transactional scope of the guard (FF-1)
///
/// The guard and the DML are emitted as consecutive statements of one
/// multi-statement rewrite that `DuckDB` re-parses and runs on the caller's
/// connection. Their atomicity — and therefore whether the guard's check is
/// snapshot-consistent with the DML — depends entirely on the caller's
/// transaction state:
///
/// * **Inside an explicit transaction** (`BEGIN … COMMIT`, or an ADBC/PG
///   connection with `autocommit = false`): every emitted statement shares
///   the one open transaction and its MVCC snapshot, so the guard's decision
///   is consistent with the DML that follows. This is the atomic path.
/// * **Under autocommit** (the default): `DuckDB` commits after *each* statement
///   of a multi-statement string, so the guard and the DML execute in
///   **separate implicit transactions**. A different connection that commits
///   in the window between them can invalidate the guard's decision:
///   - concurrent DROP — both droppers' existence guards pass, both DELETEs
///     run; the loser's DELETE matches 0 rows and reports success having
///     deleted nothing (a silent no-op, not an error);
///   - concurrent RENAME — the loser's collision guard passes, then the
///     UPDATE hits `DuckDB`'s primary-key constraint and surfaces a raw
///     `Constraint Error: Duplicate key` instead of the friendly
///     `already exists` wording.
///
/// This guard window is accepted debt (TECH-DEBT #27), the DROP/ALTER sibling
/// of the CREATE race in #23. It is **not** closed by wrapping the rewrite in
/// an emitted `BEGIN … COMMIT`: `DuckDB` rejects a nested `BEGIN` (`cannot start
/// a transaction within a transaction`), so that wrapper would fail outright
/// whenever the caller is already in a transaction, and an emitted `COMMIT`
/// would prematurely commit an `autocommit = false` caller's in-flight work —
/// breaking the very transaction-participation contract the native-DML rewrite
/// exists to provide. Callers needing atomic check-and-write should wrap their
/// own DDL in `BEGIN … COMMIT` (the atomic path above).
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
pub(crate) fn definitions_table_guard_select(name: &SqlLit) -> String {
    format!(
        "SELECT CASE \
              WHEN NOT EXISTS (SELECT 1 FROM information_schema.tables \
                                WHERE table_schema = '{DEFINITIONS_SCHEMA}' \
                                  AND table_name = '{DEFINITIONS_TABLE_NAME}') \
                THEN error('semantic view ''{name}'' does not exist') \
              ELSE TRUE \
            END"
    )
}

#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn existence_guard_select(name: &SqlLit) -> String {
    format!(
        "SELECT CASE WHEN NOT EXISTS \
                   (SELECT 1 FROM {DEFINITIONS_TABLE} WHERE name = '{name}') \
                THEN error('semantic view ''{name}'' does not exist') \
                ELSE TRUE END"
    )
}

/// Build the "target name must NOT already exist" guard for ALTER RENAME.
/// Errors with `semantic view '<new_name>' already exists` if a row with
/// the new name is found in `semantic_layer._definitions`. Runs as a
/// statement of the rewrite preceding the UPDATE; its EXISTS check is
/// snapshot-consistent with the UPDATE only within an explicit caller
/// transaction — see the transactional-scope note on
/// [`existence_guard_select`] (FF-1 / TECH-DEBT #27) for the autocommit
/// guard window (a concurrent committer can take the target name between the
/// guard and the UPDATE, surfacing a raw PK constraint error).
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn rename_collision_guard_select(new_name: &SqlLit) -> String {
    format!(
        "SELECT CASE WHEN EXISTS \
                   (SELECT 1 FROM {DEFINITIONS_TABLE} WHERE name = '{new_name}') \
                THEN error('semantic view ''{new_name}'' already exists') \
                ELSE TRUE END"
    )
}

/// Build the single-catalog guard prepended to every write DDL (FF-3).
///
/// Semantic views are single-catalog: `semantic_layer._definitions` is created
/// only in the database the extension was loaded into (the primary), and every
/// read runs on a fresh per-call connection that resolves against that primary
/// catalog. A write issued while the caller is `USE`-d into a different (e.g.
/// attached) database resolves `semantic_layer._definitions` against that other
/// catalog. In the common case that catalog has no `semantic_layer` schema, so
/// the write would otherwise fail with a cryptic
/// `schema semantic_layer does not exist` (CREATE) or a misleading
/// "does not exist" (DROP/ALTER).
///
/// This guard turns that into an actionable single-catalog error. It fires when
/// a semantic-view catalog exists in SOME OTHER database but NOT the current one
/// — exactly the "USE-d into the wrong database, and this database has no
/// catalog" case. It is a no-op on the normal single-catalog path (the current
/// database holds the catalog) and on a fresh / never-bootstrapped DB (no
/// catalog in any database — the existing table/row guards handle that). It uses
/// `duckdb_tables()`, which spans every attached catalog, rather than
/// `information_schema.tables`, which only sees the current one.
///
/// Residual (documented single-catalog limitation — TECH-DEBT #26): if the
/// attached database the caller is `USE`-d into ALSO has its own
/// `semantic_layer._definitions` (e.g. it was itself bootstrapped as a primary
/// at some point), the guard does NOT fire — the write lands in that catalog
/// while the primary-pinned reads never see it. Detecting this requires knowing
/// which catalog the read binds use, which is not exposed on the caller's
/// connection; fully closing it is the reader-context-threading work tracked as
/// AR-6 (see TECH-DEBT #26). Managing two independent semantic-view catalogs
/// from one session is unsupported until then.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn managed_catalog_guard_select() -> String {
    format!(
        "SELECT CASE \
              WHEN EXISTS (SELECT 1 FROM duckdb_tables() \
                            WHERE schema_name = '{DEFINITIONS_SCHEMA}' \
                              AND table_name = '{DEFINITIONS_TABLE_NAME}' \
                              AND database_name <> current_database()) \
               AND NOT EXISTS (SELECT 1 FROM duckdb_tables() \
                            WHERE schema_name = '{DEFINITIONS_SCHEMA}' \
                              AND table_name = '{DEFINITIONS_TABLE_NAME}' \
                              AND database_name = current_database()) \
                THEN error('semantic_views: semantic-view DDL was issued against database ''' \
                           || current_database() || \
                           ''', but the semantic view catalog lives in a different database. \
                           Semantic views are single-catalog: manage them from the database the \
                           extension was loaded into, without USE-ing into an attached database.') \
              ELSE TRUE \
            END"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existence_guard_select_emits_not_exists_and_error() {
        let g = existence_guard_select(&SqlLit::escape("sales"));
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
        let g = definitions_table_guard_select(&SqlLit::escape("sales"));
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
        let g = definitions_table_guard_select(&SqlLit::escape("O'Brien"));
        assert!(
            g.contains("error('semantic view ''O''Brien'' does not exist')"),
            "error message wrong: {g}"
        );
    }

    #[test]
    fn existence_guard_select_doubles_quotes_in_name() {
        // SqlLit::escape doubles the single quote; embedding it inside
        // an outer SQL string literal preserves correct decoding (DuckDB
        // sees ''X'' as 'X' in the literal). The user-facing error message
        // must read: semantic view 'O'Brien' does not exist.
        let g = existence_guard_select(&SqlLit::escape("O'Brien"));
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
        let g = rename_collision_guard_select(&SqlLit::escape("taken"));
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

    #[test]
    fn managed_catalog_guard_detects_cross_catalog_via_duckdb_tables() {
        // FF-3: the single-catalog guard must span catalogs (duckdb_tables, not
        // information_schema), fire only when the catalog lives in ANOTHER
        // database than the current one, and carry an actionable single-catalog
        // message that names the current database.
        let g = managed_catalog_guard_select();
        assert!(
            g.contains("FROM duckdb_tables()"),
            "must use duckdb_tables() (spans catalogs), not information_schema: {g}"
        );
        assert!(
            g.contains("database_name <> current_database()")
                && g.contains("database_name = current_database()"),
            "must compare the catalog's database against the current one: {g}"
        );
        // `duckdb_tables()` exposes `schema_name`, not `table_schema`.
        assert!(
            g.contains("schema_name = 'semantic_layer'")
                && g.contains("table_name = '_definitions'"),
            "must match the semantic_layer._definitions catalog table: {g}"
        );
        assert!(
            g.contains("single-catalog") && g.contains("|| current_database() ||"),
            "message must name the current database and state the single-catalog rule: {g}"
        );
        assert!(g.trim_start().starts_with("SELECT "), "not a SELECT: {g}");
        assert!(!g.contains(';'), "guard must not include ';' itself: {g}");
    }
}
