//! Native-SQL emission for the `parser_override` DDL path (AR-1).
//!
//! Every recognised CREATE / DROP / ALTER semantic-view statement is rewritten
//! here into native `INSERT` / `DELETE` / `UPDATE` against
//! `semantic_layer._definitions` (plus pure-SQL existence/collision guards), so
//! the writes run on the caller's connection inside the caller's transaction.
//! Read-side DDL (DESCRIBE / SHOW) is passed through unchanged by
//! [`rewrite_to_native_sql`], the dispatch entry point.
//!
//! The SQL-string escape pair and the guard-SELECT builders are compiled
//! unconditionally (no `extension` gate) so `cargo test` can exercise the
//! escaping and guard wording without linking the loadable-extension stubs;
//! the emission/rewrite functions that consume an `OverrideContext` are
//! `extension`-gated. `rewrite_to_native_sql` is re-exported from the parent
//! module for the FFI entry points; the escape/guard helpers are re-exported
//! under `#[cfg(test)]` for the parent module's unit tests.

#[cfg(feature = "extension")]
use super::{plan_rewrite, OverrideContext, RewriteAction};
#[cfg(feature = "extension")]
use crate::catalog::writes::{
    definitions_table_guard_select, existence_guard_select, rename_collision_guard_select,
};
#[cfg(feature = "extension")]
use crate::catalog::DEFINITIONS_TABLE;
#[cfg(feature = "extension")]
use crate::errors::ParseError;
#[cfg(feature = "extension")]
use crate::ident::normalize_view_name;

// ---------------------------------------------------------------------------
// v0.8.x: native-SQL rewrite for parser_override (transactional DDL)
// ---------------------------------------------------------------------------
//
// `parser_override` is the sole semantic-view DDL entry point. Every recognised
// statement is rewritten here and re-executed on the caller's connection by
// DuckDB — the legacy parse_function / sv_ddl_internal fallback was retired
// in v0.8.0.
//
// Rewriting is dispatched by shape:
//
//   * CREATE / CREATE OR REPLACE / CREATE IF NOT EXISTS
//     CREATE ... FROM YAML FILE '/path/...'
//     DROP / DROP IF EXISTS
//     ALTER ... RENAME TO / SET COMMENT / UNSET COMMENT
//       → emitted as native INSERT / DELETE / UPDATE against
//         `semantic_layer._definitions`, so writes participate in the
//         caller's transaction (the v0.8.0 ADBC autocommit=false fix).
//
//   * DESCRIBE / SHOW SEMANTIC * / GET_DDL / READ_YAML_FROM_SEMANTIC_VIEW
//       → passed through as `SELECT * FROM <existing_read_side_table_function>(...)`
//         (or the same SQL with WHERE/LIMIT clauses appended). DuckDB re-parses
//         and executes on the caller's connection. The read-side table functions
//         themselves still query via `catalog_conn` (committed state); making
//         DESCRIBE/SHOW transactional w.r.t. the caller's snapshot would require
//         exposing the executing connection from BindInfo — a separate refactor.
//
//   * Anything else (`validate_and_rewrite` returns None)
//       → `Ok(None)`; the C++ shim returns DISPLAY_ORIGINAL_ERROR and DuckDB's
//         default parser handles it.
//
// CREATE/DROP/ALTER need a `CatalogReader` for existence checks; CREATE also
// runs catalog-side queries during enrichment (PK lookup, LIMIT 0 type
// inference, fact typing). Phase 62 attaches the `OverrideContext` directly
// to the C++ `SemanticViewsParserInfo` via an opaque `Box`, so the rewriters
// take `&OverrideContext` here instead of looking up by token. Under
// `cargo test` (no extension feature) these code paths are excluded entirely
// (this entry point itself is feature-gated; its sole caller —
// `sv_parser_override_rust` — is `extension`-only).
//
// INVARIANT (AR-5) — purity / idempotence. This function MUST be a pure
// function of `query` (and committed catalog state): for a given input it
// must produce the same `Ok(Some)` / `Ok(None)` / `Err(message, position)`
// result on every call, with no dependence on call order, wall-clock time,
// `HashMap` iteration order, or any mutable process state. The error-reporting
// layer depends on this: after the override path runs, DuckDB's failed default
// parser drives `sv_parse_function_rust`, which calls this function a SECOND
// time (via `run_validation_for_parse_function`) purely to recover the same
// `Err` message and caret position the override produced. If the two runs can
// diverge, the caret error shown to the user no longer matches the rewrite
// that actually ran. Any future change that reads mutable state or introduces
// nondeterminism here breaks that contract and must instead cache the first
// run's `(query -> result)` rather than re-deriving it.
#[cfg(feature = "extension")]
pub(crate) fn rewrite_to_native_sql(
    _ctx: &OverrideContext,
    query: &str,
) -> Result<Option<String>, ParseError> {
    let Some(action) = plan_rewrite(query)? else {
        return Ok(None);
    };

    match action {
        // CREATE from an in-memory definition — hand the definition straight to
        // the shared emission path. AR-2: no JSON serialize → re-parse →
        // deserialize round-trip; the `SemanticViewDefinition` flows structurally.
        RewriteAction::Create { name, def, mode } => {
            emit_native_create_sql(&name, *def, mode.or_replace(), mode.if_not_exists())
        }
        // CREATE FROM YAML FILE — emit the INSERT that selects from the
        // `__sv_compute_create_from_yaml` helper TF (which reads the file at
        // execution). AR-2: no `\x01`-delimited sentinel string.
        RewriteAction::CreateFromYamlFile {
            file_path,
            name,
            comment,
            mode,
        } => emit_native_create_from_yaml_file(
            &file_path,
            &name,
            &comment,
            mode.or_replace(),
            mode.if_not_exists(),
        ),
        // DROP / ALTER: pure-SQL race-guard + native DML on the caller's
        // connection. Names/comment are carried raw; escape at the boundary so
        // the emission helpers keep receiving already-escaped args.
        RewriteAction::Drop { name, if_exists } => rewrite_drop(&escape_sql_arg(&name), if_exists),
        RewriteAction::AlterRename {
            name,
            new_name,
            if_exists,
        } => rewrite_alter_rename(
            &escape_sql_arg(&name),
            &escape_sql_arg(&new_name),
            if_exists,
        ),
        RewriteAction::AlterSetComment {
            name,
            comment,
            if_exists,
        } => rewrite_alter_comment(
            &escape_sql_arg(&name),
            Some(&escape_sql_arg(&comment)),
            if_exists,
        ),
        RewriteAction::AlterUnsetComment { name, if_exists } => {
            rewrite_alter_comment(&escape_sql_arg(&name), None, if_exists)
        }
        // Read-side DDL (DESCRIBE / SHOW / SHOW COLUMNS): DuckDB runs the
        // read-side table function on the caller's connection unchanged.
        RewriteAction::Passthrough(sql) => Ok(Some(sql)),
    }
}

/// Shared CREATE-emission helper for the in-memory-definition path
/// (`RewriteAction::Create`). The FROM YAML FILE path uses the sibling
/// `emit_native_create_from_yaml_file`.
///
/// Steps (Phase 65 Plan 06 — pure-SQL):
/// 1. Run `enrich_definition_for_create` (validation + graph + serialize
///    to JSON; no catalog connection needed).
/// 2. Emit `INSERT [OR REPLACE / OR IGNORE] INTO semantic_layer._definitions
///    ... RETURNING name AS view_name` so DuckDB executes the write on the
///    caller's connection inside the caller's transaction. The plain CREATE
///    form (no OR REPLACE, no IF NOT EXISTS) wraps the INSERT in a
///    CASE+error subquery that emits "already exists" wording — replaces
///    the pre-Plan-06 `catalog.exists()` Rust-side pre-check.
///
/// For IF NOT EXISTS on an already-existing view, `INSERT OR IGNORE`
/// absorbs the duplicate silently — equivalent shape to the legacy
/// `SELECT ... WHERE 1 = 0` fast path (zero rows returned).
#[cfg(feature = "extension")]
fn emit_native_create_sql(
    name: &str,
    def: crate::model::SemanticViewDefinition,
    or_replace: bool,
    if_not_exists: bool,
) -> Result<Option<String>, ParseError> {
    // Defensive validation — `name` arrives already normalised (bare,
    // case-folded if it was unquoted) from validate_create_body via the
    // `RewriteAction::Create` it produced. Re-quote before re-normalising so
    // this pass is a true no-op on normalised input: normalising the BARE name
    // again would fold a case-preserved quoted name (`"SalesView"` →
    // `salesview`, PA-8) and split a dotted name (`"a.b"` → `b`).
    let name = normalize_view_name(&crate::expand::quote_ident(name)).map_err(|e| ParseError {
        message: format!("Invalid view name: {e}"),
        position: None,
    })?;
    let name_escaped = escape_sql_arg(&name);

    // Phase 65 (D-16, metadata-via-SQL): enrichment no longer takes a
    // catalog connection. CREATE-time `now()` / `current_database()` /
    // `current_schema()` capture is embedded as SQL inside the emitted
    // INSERT via `json_merge_patch` so it resolves on the CALLER's
    // connection at INSERT-time, preserving D-21 transactional contract
    // without parser_override holding a long-lived handle. CREATE-time
    // column type inference (`column_type_names`, fact `output_type`)
    // is deferred to read-side bind under Plan 05's C++ Catalog API
    // migration (D-17).
    let enriched_json =
        crate::ddl::define::enrich_definition_for_create(&name, def).map_err(|e| ParseError {
            message: e,
            position: None,
        })?;
    let enriched_escaped = escape_sql_arg(&enriched_json);

    // Metadata-via-SQL sub-expression: produces a VARCHAR by patching
    // the enriched JSON (no created_on / database_name / schema_name
    // fields populated by the Rust side) with the now()/current_database()
    // /current_schema() values resolved on the caller's connection.
    //
    // RFC-7396 semantics: json_merge_patch overrides any keys present in
    // the patch. Phase 39 metadata behaviour is preserved because the
    // enriched JSON omits the three metadata keys (Vec::is_empty /
    // Option::is_none skip_serializing) so the patch is the sole source.
    // AR-4: stamp the storage-format version alongside the metadata so every
    // freshly written row records `schema_version`. It is injected here (not
    // carried on the struct) so it never leaks into YAML export.
    let schema_version = crate::model::CURRENT_SCHEMA_VERSION;
    let metadata_patched_definition = format!(
        "json_merge_patch( \
            '{enriched_escaped}'::JSON, \
            json_object( \
              'created_on', strftime(now(), '%Y-%m-%dT%H:%M:%SZ'), \
              'database_name', current_database(), \
              'schema_name', current_schema(), \
              'schema_version', {schema_version} \
            ) \
         )::VARCHAR"
    );

    // The generated SQL runs on the caller's connection, so its EXISTS
    // subqueries see in-flight INSERTs from the same transaction. Three
    // shapes:
    //   - OR REPLACE: straight INSERT OR REPLACE, no guard needed.
    //   - IF NOT EXISTS: INSERT OR IGNORE absorbs same-snapshot duplicates
    //     (the same-txn duplicate path, mirroring the SELECT WHERE 1=0
    //     fast path on committed-state hits). It does *not* paper over
    //     a cross-connection committer race: two transactions that each
    //     see no row will both INSERT, and DuckDB's PK constraint raises
    //     a write-write conflict on the second commit. That matches plain
    //     CREATE concurrency semantics — see TECH-DEBT item 23.
    //   - Plain CREATE: CASE+error() raises the friendly "already exists"
    //     message before the INSERT can fire, replacing what would
    //     otherwise be a generic PK constraint violation. Phase 65: the
    //     parser-side `ctx.catalog.exists` pre-check above is the
    //     committed-state fast path; the CASE inside the INSERT is the
    //     same-transaction guard.
    let sql = if or_replace {
        format!(
            "INSERT OR REPLACE INTO {DEFINITIONS_TABLE} (name, definition) \
             VALUES ('{name_escaped}', {metadata_patched_definition}) \
             RETURNING name AS view_name"
        )
    } else if if_not_exists {
        format!(
            "INSERT OR IGNORE INTO {DEFINITIONS_TABLE} (name, definition) \
             VALUES ('{name_escaped}', {metadata_patched_definition}) \
             RETURNING name AS view_name"
        )
    } else {
        format!(
            "INSERT INTO {DEFINITIONS_TABLE} (name, definition) \
             SELECT \
               CASE WHEN EXISTS (SELECT 1 FROM {DEFINITIONS_TABLE} \
                                 WHERE name = '{name_escaped}') \
                    THEN error('semantic view ''{name_escaped}'' already exists; \
                                use CREATE OR REPLACE SEMANTIC VIEW to overwrite') \
                    ELSE '{name_escaped}' \
               END, \
               {metadata_patched_definition} \
             RETURNING name AS view_name"
        )
    };
    Ok(Some(sql))
}

/// Read the FROM YAML FILE sentinel produced by `rewrite_ddl_yaml_file_body`
/// and emit a transactional INSERT that selects from the
/// `__sv_compute_create_from_yaml(path, name, kind, comment)` helper TF
/// (registered via the C++ Catalog API in `cpp/src/shim.cpp`). The helper's
/// bind callback opens a per-call `Connection(*context.db)`, runs
/// `read_text()` against the user-supplied path, calls into Rust to parse
/// and enrich the YAML, and returns a metadata-less JSON in a single row.
/// The outer INSERT wraps that row with `json_merge_patch` to add the
/// metadata fields (`created_on`, `database_name`, `schema_name`) on the
/// caller's connection -- matching `emit_native_create_sql`'s non-YAML
/// behaviour byte-for-byte.
///
/// Phase 65 Plan 06: pure-SQL, no `OverrideContext` consumed. The YAML
/// read happens inside the `__sv_compute_create_from_yaml` helper TF's
/// bind callback (per-call `Connection(*context.db)`), not on any
/// long-lived extension-owned connection.
#[cfg(feature = "extension")]
fn emit_native_create_from_yaml_file(
    file_path: &str,
    name: &str,
    comment: &str,
    or_replace: bool,
    if_not_exists: bool,
) -> Result<Option<String>, ParseError> {
    // Phase 65.1 Plan 07 (IN-04 D-24): `kind` is not threaded into the helper
    // TF — the outer INSERT shape (OR IGNORE / OR REPLACE / plain) already
    // encodes the ON CONFLICT behaviour, chosen from `or_replace`/`if_not_exists`.

    // Defensive validation of the name (matches emit_native_create_sql):
    // re-quote before re-normalising so the pass is a no-op on the
    // already-normalised bare name — re-normalising it bare would fold a
    // case-preserved quoted name (PA-8) or split a dotted one.
    let name = normalize_view_name(&crate::expand::quote_ident(name)).map_err(|e| ParseError {
        message: format!("Invalid view name: {e}"),
        position: None,
    })?;
    let name_escaped = escape_sql_arg(&name);
    let path_escaped = escape_sql_arg(file_path);
    let comment_escaped = escape_sql_arg(comment);

    // Helper-TF subquery + metadata-via-SQL wrapper. The helper TF returns
    // exactly one row whose `new_def` column contains the metadata-less
    // enriched JSON. We patch in the metadata fields on the caller's
    // connection so they reflect the user's session (matches Plan 03's
    // non-YAML CREATE behaviour byte-for-byte).
    //
    // RFC-7396 semantics (verified by Plan 04 Wave 0 spike): json_merge_patch
    // overrides keys present in the patch. The helper TF's new_def omits the
    // three metadata keys (skip_serializing_if on the struct), so the patch
    // is the sole source -- no risk of overwriting a user-supplied value.
    // AR-4: stamp schema_version alongside the metadata (see the inline-CREATE
    // sibling above). Injected here rather than carried on the struct so it
    // stays out of YAML export.
    let metadata_patched = format!(
        "json_merge_patch( \
            new_def::JSON, \
            json_object( \
              'created_on', strftime(now(), '%Y-%m-%dT%H:%M:%SZ'), \
              'database_name', current_database(), \
              'schema_name', current_schema(), \
              'schema_version', {schema_version} \
            ) \
         )::VARCHAR",
        schema_version = crate::model::CURRENT_SCHEMA_VERSION
    );
    let helper_from = format!(
        "FROM __sv_compute_create_from_yaml('{path_escaped}', \
            '{name_escaped}', '{comment_escaped}')"
    );

    // Three INSERT shapes mirror the inline CREATE path
    // (emit_native_create_sql):
    //   OR REPLACE     : INSERT OR REPLACE -- no friendly-error guard needed.
    //   IF NOT EXISTS  : INSERT OR IGNORE absorbs same-snapshot duplicates.
    //   Plain          : CASE+error guard inside SELECT raises the friendly
    //                    "already exists" message before the INSERT can fire
    //                    (Phase 60 race-guard pattern carried forward).
    let sql = if or_replace {
        format!(
            "INSERT OR REPLACE INTO {DEFINITIONS_TABLE} (name, definition) \
             SELECT '{name_escaped}', {metadata_patched} \
             {helper_from} \
             RETURNING name AS view_name"
        )
    } else if if_not_exists {
        format!(
            "INSERT OR IGNORE INTO {DEFINITIONS_TABLE} (name, definition) \
             SELECT '{name_escaped}', {metadata_patched} \
             {helper_from} \
             RETURNING name AS view_name"
        )
    } else {
        format!(
            "INSERT INTO {DEFINITIONS_TABLE} (name, definition) \
             SELECT \
               CASE WHEN EXISTS (SELECT 1 FROM {DEFINITIONS_TABLE} \
                                 WHERE name = '{name_escaped}') \
                    THEN error('semantic view ''{name_escaped}'' already exists; \
                                use CREATE OR REPLACE SEMANTIC VIEW to overwrite') \
                    ELSE '{name_escaped}' \
               END, \
               {metadata_patched} \
             {helper_from} \
             RETURNING name AS view_name"
        )
    };
    Ok(Some(sql))
}

// SQL-string escape helpers (round-trip pair).
//
// `escape_sql_arg` doubles single quotes so the input can be embedded inside
// a single-quoted SQL string literal: `O'Brien` → `O''Brien`. `unescape_sql_arg`
// reverses the doubling for values that arrived already-escaped (e.g. the
// SET COMMENT literal that `rewrite_alter_comment` re-parses as JSON).
//
// The pair is unconditionally compiled (no `#[cfg(feature = "extension")]`)
// so unit tests under `cargo test` can exercise the escaping rules without
// linking the loadable-extension stubs. They have no FFI dependencies.

/// Undo the SQL `''`-escaping of an already-escaped single-quoted-literal value.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn unescape_sql_arg(s: &str) -> String {
    s.replace("''", "'")
}

/// Re-escape a string for embedding in single-quoted SQL.
#[cfg_attr(not(any(feature = "extension", test)), allow(dead_code))]
pub(crate) fn escape_sql_arg(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(feature = "extension")]
fn rewrite_drop(name_escaped: &str, if_exists: bool) -> Result<Option<String>, ParseError> {
    if if_exists {
        // IF EXISTS: pure DELETE on the caller's connection — affects 0
        // rows when the view is missing (silent no-op contract).
        //
        // Phase 65.1 Plan 04 (WR-03): prepend a `definitions_table_guard`
        // so the DELETE never binds against a missing
        // `semantic_layer._definitions` on a never-bootstrapped RO DB
        // (which would otherwise leak `Catalog Error: Table
        // _definitions does not exist`). When the table is missing the
        // guard errors with the canonical "does not exist" wording and
        // the DELETE is never bound (per-statement lazy bind — see
        // `definitions_table_guard_select` docs). The silent-no-op
        // contract for missing-row-but-table-present is preserved by
        // the DELETE's 0-row effect.
        let table_guard = definitions_table_guard_select(name_escaped);
        return Ok(Some(format!(
            "{table_guard}; \
             DELETE FROM {DEFINITIONS_TABLE} WHERE name = '{name_escaped}' \
             RETURNING name AS view_name"
        )));
    }

    // Plain DROP: pure-SQL existence guard + DELETE on the caller's
    // connection. The guard runs in the same transaction as the DELETE so
    // its NOT EXISTS check is snapshot-consistent. Phase 65 Plan 06: the
    // legacy `catalog.exists()` Rust-side pre-check is gone — H1
    // catalog_conn retired; the guard subsumes both the never-existed
    // case and the concurrent-drop case under a single "does not exist"
    // wording.
    //
    // Phase 65.1 Plan 04 (WR-03): prepend a `definitions_table_guard` so
    // neither the row-existence guard NOR the DELETE bind against a
    // missing `semantic_layer._definitions` on a never-bootstrapped RO
    // DB. Three-statement form: <table_guard>; <row_guard>; <DELETE>.
    // First statement errors → second and third never bind.
    let table_guard = definitions_table_guard_select(name_escaped);
    let guard = existence_guard_select(name_escaped);
    Ok(Some(format!(
        "{table_guard}; \
         {guard}; \
         DELETE FROM {DEFINITIONS_TABLE} WHERE name = '{name_escaped}' \
         RETURNING name AS view_name"
    )))
}

#[cfg(feature = "extension")]
fn rewrite_alter_rename(
    old_escaped: &str,
    new_escaped: &str,
    if_exists: bool,
) -> Result<Option<String>, ParseError> {
    if if_exists {
        // IF EXISTS: pure UPDATE on the caller's connection. We still need
        // the rename-collision guard (target name must not be taken),
        // because PK violations from DuckDB's UPDATE produce a less
        // actionable error message. The guard runs in the same transaction
        // as the UPDATE so the EXISTS check is snapshot-consistent. The
        // UPDATE itself silently affects 0 rows on a missing source row —
        // matches the IF EXISTS contract.
        //
        // Phase 65.1 Plan 04 (WR-03): prepend a `definitions_table_guard`
        // so neither the collision guard NOR the UPDATE bind against a
        // missing `semantic_layer._definitions` on a never-bootstrapped
        // RO DB.
        let table_guard = definitions_table_guard_select(old_escaped);
        let collision_guard = rename_collision_guard_select(new_escaped);
        return Ok(Some(format!(
            "{table_guard}; \
             {collision_guard}; \
             UPDATE {DEFINITIONS_TABLE} SET name = '{new_escaped}' \
             WHERE name = '{old_escaped}' \
             RETURNING '{old_escaped}'::VARCHAR AS old_name, name AS new_name"
        )));
    }

    // Plain ALTER RENAME: pure-SQL existence guard (source must exist) +
    // collision guard (target must not exist) + UPDATE. All three run on
    // the caller's connection in the same transaction so the EXISTS
    // checks are snapshot-consistent with the DML. Phase 65 Plan 06: the
    // legacy `catalog.exists()` Rust-side pre-checks are gone.
    //
    // Phase 65.1 Plan 04 (WR-03): prepend a `definitions_table_guard` so
    // none of the row guards / UPDATE bind against a missing
    // `semantic_layer._definitions` on a never-bootstrapped RO DB.
    let table_guard = definitions_table_guard_select(old_escaped);
    let exist_guard = existence_guard_select(old_escaped);
    let collision_guard = rename_collision_guard_select(new_escaped);
    Ok(Some(format!(
        "{table_guard}; \
         {exist_guard}; \
         {collision_guard}; \
         UPDATE {DEFINITIONS_TABLE} SET name = '{new_escaped}' \
         WHERE name = '{old_escaped}' \
         RETURNING '{old_escaped}'::VARCHAR AS old_name, name AS new_name"
    )))
}

#[cfg(feature = "extension")]
fn rewrite_alter_comment(
    name_escaped: &str,
    new_comment_escaped: Option<&str>,
    if_exists: bool,
) -> Result<Option<String>, ParseError> {
    // Phase 65 Plan 06 — all pure-SQL on the caller's connection:
    //   - ALTER SET/UNSET COMMENT uses json_merge_patch (Plan 04 Wave 0
    //     spike confirmed DuckDB v1.5.2 honors RFC-7396 null-as-delete).
    //   - Existence is enforced by the existence_guard_select preceding
    //     the UPDATE (plain ALTER) — replaces the legacy `catalog.exists()`
    //     Rust-side pre-check. IF EXISTS uses a plain UPDATE that affects
    //     0 rows on a missing source.
    //
    // The legacy "does not exist" wording is preserved by
    // existence_guard_select — matches phase45's expectations
    // byte-for-byte.

    // Build the json_merge_patch patch literal.
    //   SET COMMENT 'new text' -> `'{"comment":"new text"}'::JSON`
    //   UNSET COMMENT          -> `'{"comment":null}'::JSON`  (RFC-7396 null-as-delete)
    //
    // For SET, we use serde_json::to_string on a one-key object so internal
    // `"` and `\` characters in the user's comment are JSON-escaped
    // correctly; then escape_sql_arg doubles any embedded single quotes for
    // the outer single-quoted SQL literal. Belt-and-braces escape: JSON
    // first (handles `"`/`\`/control chars), SQL second (handles `'`).
    let (patch_json_for_sql, status_label) =
        match new_comment_escaped {
            Some(escaped) => {
                // The arg arrives SQL-escaped (single quotes doubled); undo
                // that before handing to serde_json so the JSON value is the
                // user's literal comment.
                let comment = unescape_sql_arg(escaped);
                let patch = serde_json::to_string(&serde_json::json!({"comment": comment}))
                    .map_err(|e| ParseError {
                        message: format!("failed to build comment patch: {e}"),
                        position: None,
                    })?;
                (escape_sql_arg(&patch), "comment set")
            }
            None => {
                // UNSET COMMENT: constant patch. The Wave 0 spike empirically
                // confirms DuckDB v1.5.2 implements RFC-7396 null-as-delete.
                (r#"{"comment":null}"#.to_string(), "comment unset")
            }
        };

    if if_exists {
        // IF EXISTS preserves its silent contract on race: pre-check saw the
        // row; if a concurrent DROP commits before our UPDATE, the UPDATE
        // simply affects 0 rows.
        //
        // Phase 65.1 Plan 04 (WR-03): prepend a `definitions_table_guard`
        // so the UPDATE never binds against a missing
        // `semantic_layer._definitions` on a never-bootstrapped RO DB
        // (which would leak `Catalog Error: Table _definitions does
        // not exist`). On missing-table the guard errors with the
        // canonical wording; on missing-row-but-table-present the
        // UPDATE's 0-row effect preserves the silent IF EXISTS contract.
        let table_guard = definitions_table_guard_select(name_escaped);
        return Ok(Some(format!(
            "{table_guard}; \
             UPDATE {DEFINITIONS_TABLE} \
                SET definition = json_merge_patch(definition::JSON, '{patch_json_for_sql}'::JSON)::VARCHAR \
              WHERE name = '{name_escaped}' \
             RETURNING name, '{status_label}'::VARCHAR AS status"
        )));
    }

    // Plain ALTER: pure-SQL existence guard + UPDATE on the caller's
    // connection. The guard's NOT EXISTS check is snapshot-consistent
    // with the UPDATE since both run in the same transaction. Concurrent
    // ALTER-against-the-same-row carries no lost-update risk because we
    // apply the mutation via json_merge_patch ON THE CURRENT ROW — not a
    // Rust-side snapshot.
    //
    // Phase 65.1 Plan 04 (WR-03): prepend a `definitions_table_guard` so
    // neither the row guard NOR the UPDATE bind against a missing
    // `semantic_layer._definitions` on a never-bootstrapped RO DB.
    let table_guard = definitions_table_guard_select(name_escaped);
    let guard = existence_guard_select(name_escaped);
    Ok(Some(format!(
        "{table_guard}; \
         {guard}; \
         UPDATE {DEFINITIONS_TABLE} \
            SET definition = json_merge_patch(definition::JSON, '{patch_json_for_sql}'::JSON)::VARCHAR \
          WHERE name = '{name_escaped}' \
         RETURNING name, '{status_label}'::VARCHAR AS status"
    )))
}
