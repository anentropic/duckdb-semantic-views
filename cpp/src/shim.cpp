// C++ helper for the DuckDB semantic_views extension.
//
// The Rust entry point (semantic_views_init_c_api, C_STRUCT ABI) owns the
// DuckDB handshake and function registration. After init, it calls
// sv_register_parser_hooks() here to install:
//
//   * parser_override (sv_parser_override) — the success path. Recognised
//     CREATE / DROP / ALTER / DESCRIBE / SHOW DDL is rewritten to native SQL
//     and re-parsed on the caller's connection (transactional behaviour).
//   * parse_function (sv_parse_stub) — Phase 62 Plan 03 error-reporting
//     layer. Called by DuckDB after the default parser fails on an
//     unrecognised prefix; re-runs validation and returns
//     DISPLAY_EXTENSION_ERROR with `error_location` so
//     ParserException::SyntaxError can render `LINE 1: … ^` (caret).
//   * plan_function (sv_plan_unreachable) — required sibling of
//     parse_function; should never fire because sv_parse_stub never returns
//     PARSE_SUCCESSFUL.
//
// All DuckDB C++ symbols are provided by compiling duckdb.cpp (the
// amalgamation source) alongside this file. Symbol visibility on the cdylib
// restricts exports to just the Rust entry point, so these definitions stay
// internal to the binary.
//
// DuckDB 1.5.0 moved the parser extension type declarations from duckdb.hpp
// into duckdb.cpp. The compat header re-declares them so this translation
// unit can use them. See cpp/include/parser_extension_compat.hpp for details.

#include "parser_extension_compat.hpp"
#include "shim.hpp"
#include <cstdint>
#include <cstring>
#include <memory>

using namespace duckdb;

// ---------------------------------------------------------------------------
// BORROW-contract bridge guard (Phase 65.1 Plan 10, WR-06, D-12)
// ---------------------------------------------------------------------------
// The `reinterpret_cast<duckdb_connection>(Connection*)` pattern used at ~20
// bind/exec callback sites in this file (e.g. shim.cpp:993, 1285, 1311, …)
// depends on `duckdb_connection` being layout-compatible with a single
// pointer. This compile-time guard catches DuckDB ABI drift that would
// change the size of `duckdb_connection` (e.g. wrapping it in a struct with
// extra fields). The complementary runtime probe in `sv_register_parser_hooks`
// catches representation drift the size check would miss (e.g. an indirection
// through `internal_ptr`).
//
// See cpp/src/shim.cpp BORROW contract docs and
// .planning/phases/65-overridecontext-connection-teardown/65-REVIEW.md WR-06.
static_assert(sizeof(duckdb_connection) == sizeof(void*),
    "duckdb_connection must be pointer-sized — bridge contract broken; "
    "see cpp/src/shim.cpp BORROW contract docs and 65-REVIEW.md WR-06 / "
    "Phase 65.1 D-12");

// ---------------------------------------------------------------------------
// Rust FFI declarations (defined in src/parse.rs)
// ---------------------------------------------------------------------------
extern "C" {
    // Parser-override DDL rewrite. For recognized semantic-view DDL emits
    // native SQL (INSERT/DELETE/UPDATE on _definitions for write-side DDL,
    // or `SELECT * FROM <read_side_table_function>(...)` pass-through for
    // DESCRIBE/SHOW). Returns:
    //   0 = success: heap-owned (ptr, len) written to *sql_out_ptr/*sql_out_len.
    //                Caller takes ownership and MUST release via sv_free_buffer.
    //                Buffer is NOT NUL-terminated — read exactly *sql_out_len bytes.
    //   1 = error:   message written to error_out (NUL-terminated, capped at
    //                error_out_len-1 bytes); *sql_out_ptr left untouched.
    //                (Phase 62: unused — Err branches now return rc=2 and
    //                let parse_function render caret via DISPLAY_EXTENSION_ERROR.)
    //   2 = not ours: defer to default parser.
    //
    // AR-7: the opaque Box<OverrideContext>* context parameter was removed.
    // It was empty after Phase 65 Plan 06 moved catalog pre-checks into the
    // emitted SQL, so the hook no longer needs any per-DB state.
    uint8_t sv_parser_override_rust(
        const char *query_ptr, size_t query_len,
        char **sql_out_ptr, size_t *sql_out_len,
        char *error_out, size_t error_out_len);

    // Phase 62 Plan 03: parse_function callback. Called by DuckDB after the
    // default parser fails on an unrecognised prefix. Re-runs validation
    // and returns rc + error message + byte-offset position so
    // ParserException::SyntaxError can render `LINE 1: ... ^`.
    // Return codes:
    //   0 = success/unreachable (defensive)
    //   1 = ours-but-invalid (validation error or near-miss). error_buf
    //       gets the message, position_out gets the byte offset (or
    //       UINT32_MAX if no position is available).
    //   2 = not ours; defer (DISPLAY_ORIGINAL_ERROR on the C++ side).
    //   3 = valid DDL but parser_override didn't fire (e.g. override
    //       setting reset by disable_peg_parser). error_buf gets an
    //       actionable hint; position_out gets 0.
    uint8_t sv_parse_function_rust(
        const char *query_ptr, size_t query_len,
        char *error_buf, size_t error_buf_len,
        uint32_t *position_out);

    // Releases a buffer previously produced by sv_parser_override_rust.
    // Safe to call with a null pointer (no-op). ptr/len must be the exact
    // pair the Rust side returned.
    void sv_free_buffer(char *ptr, size_t len);

    // Phase 65 Plan 04 (Task 2 Step C) — Rust FFI bridge for the
    // `__sv_compute_create_from_yaml` helper TF. Reads file content from
    // the bind callback (via Connection probe(*context.db) + read_text(?))
    // and parses + enriches the YAML on the Rust side, returning the
    // metadata-less JSON definition as a heap-owned (ptr, len) buffer.
    //
    // The outer parser_override INSERT wraps `new_def` with json_merge_patch
    // to add now()/current_database()/current_schema() on the caller's
    // connection (matching the metadata-via-SQL pattern landed in Plan 03
    // for the inline CREATE path).
    //
    // Parameters:
    //   content_ptr/len — YAML bytes loaded by the C++ bind callback.
    //   name_ptr/len    — view name (bare identifier).
    //   comment_ptr/len — optional COMMENT='...' value; pass len=0 for
    //                     "no comment provided" (the helper leaves
    //                     `def.comment` untouched in that case so the
    //                     YAML's own `comment:` field, if any, survives).
    //   out_ptr/out_len — on rc=0, point to a heap-owned UTF-8 buffer
    //                     (NOT NUL-terminated) holding the JSON definition.
    //                     Caller MUST release via `sv_free_buffer` with
    //                     the exact (ptr, len) pair Rust returned.
    //   error_buf       — on rc!=0, gets a NUL-terminated error message
    //                     capped at `error_buf_len-1` bytes. Untouched
    //                     on success.
    //
    // Return codes:
    //   0 — success; (out_ptr, out_len) populated.
    //   1 — YAML parse / size-cap error; error_buf populated.
    //   2 — enrichment / validation error; error_buf populated.
    //   3 — internal error (panic across FFI, allocation failure, etc.);
    //       error_buf populated.
    // Phase 65.1 Plan 07 (IN-04 D-24): `kind` parameter removed — the outer
    // parser_override INSERT shape (OR IGNORE / OR REPLACE / plain) already
    // encodes ON CONFLICT behaviour, so the helper itself does not branch on
    // it. Three-arg helper TF signature matches: (file_path, view_name,
    // comment).
    uint8_t sv_compute_create_from_yaml_rust(
        const uint8_t *content_ptr, size_t content_len,
        const uint8_t *name_ptr, size_t name_len,
        const uint8_t *comment_ptr, size_t comment_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 (Task 1 / Wave 0 bridge spike) — Rust dispatcher for
    // `list_semantic_views()`. The C++ bind callback opens a per-call
    // `Connection probe(*context.db)` and bridges by casting the stack
    // `Connection *` to `duckdb_connection` (cast confirmed by
    // duckdb.cpp:266432-266447 where `duckdb_connect` is literally
    // `reinterpret_cast<duckdb_connection>(new Connection(...))`, so the
    // C-API handle is a borrowed pointer to a C++ `Connection`).
    //
    // The bridge is a BORROW, not a transfer: the Rust dispatcher does NOT
    // call `duckdb_disconnect` (which would `delete` the stack Connection
    // and segfault). When the C++ bind scope ends, the `probe` local's
    // destructor runs and `~Connection()` does the correct teardown.
    //
    // The dispatcher serializes the result rows into a length-prefixed
    // binary buffer (`u32 row_count; for each row: for each of 6 cols: u32
    // byte_len; bytes...`) which the C++ bind parses into BindData. The
    // exec callback then emits rows from BindData. Using a flat binary
    // wire format avoids needing matched struct layouts across the FFI
    // boundary.
    //
    // Return codes:
    //   0 — success; (out_ptr, out_len) populated with the binary buffer.
    //        Caller MUST release via `sv_free_buffer`.
    //   1 — catalog read error; error_buf populated.
    //   2 — internal error (panic across FFI); error_buf populated.
    uint8_t sv_list_semantic_views_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 Task 2 (Wave 1) — Rust dispatcher for the migrated
    // `list_terse_semantic_views()` table function. 5-column subset of
    // list_semantic_views (no `comment`). Same bridge mechanism and borrow
    // contract as the Wave 0 spike.
    uint8_t sv_list_terse_semantic_views_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 Task 2 (Wave 1) — Rust dispatchers for the migrated
    // zero-arg "_all" TFs. All emit homogeneous VARCHAR rows; cell layout
    // matches the matching legacy duckdb-rs VTab. See per-dispatcher Rust
    // doc-headers for the exact column order.
    uint8_t sv_show_semantic_dimensions_all_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_metrics_all_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_facts_all_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_materializations_all_bind_rust(
        duckdb_connection conn,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 Task 3 (Wave 2) — Rust dispatchers for the migrated
    // single-arg (view name) and two-arg (view name + metric name) TFs.
    // All take a per-call borrowed duckdb_connection plus name (ptr, len)
    // tuples. The single-view show_columns / describe / show_semantic_*
    // variants emit VARCHAR rows; show_semantic_dimensions_for_metric
    // emits VARCHAR+BOOL rows (3 VARCHAR + 1 trailing BOOL per row).
    uint8_t sv_show_columns_in_semantic_view_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_describe_semantic_view_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_dimensions_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_metrics_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_facts_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_materializations_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_show_semantic_dimensions_for_metric_bind_rust(
        duckdb_connection conn,
        const uint8_t *view_name_ptr, size_t view_name_len,
        const uint8_t *metric_name_ptr, size_t metric_name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 Task 4 (Wave 3) — Rust dispatchers for the migrated
    // scalar functions (get_ddl, read_yaml_from_semantic_view). Per-row
    // dispatch: each invocation takes the borrowed per-call duckdb_connection
    // (opened from the C++ exec callback via Connection probe(*state.GetContext().db))
    // plus the input string args, and returns a heap-owned UTF-8 buffer for
    // the C++ side to copy into the result Vector via StringVector::AddString.
    //
    // Same wire convention as the bind dispatchers: rc=0 on success with
    // (out_ptr, out_len) populated (caller frees via sv_free_buffer), rc=1
    // on user-visible error (error_buf populated, raised as
    // InvalidInputException by the C++ side), rc=2 on internal panic.
    uint8_t sv_get_ddl_exec_rust(
        duckdb_connection conn,
        const uint8_t *type_ptr, size_t type_len,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
    uint8_t sv_read_yaml_from_semantic_view_exec_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 Task 5 (Wave 5) — Rust dispatcher for the migrated
    // `explain_semantic_view(view_name, dimensions := [...], metrics := [...],
    // facts := [...])` table function. Same per-call Connection BORROW
    // contract as the 14 Batch-1 migrations. The three optional named
    // LIST(VARCHAR) parameters are flattened on the C++ side into the
    // standard length-prefixed wire format (`u32 count; for each entry:
    // u32 len + bytes`) and passed as (ptr, len) pairs. A null pointer
    // with len=0 means the named parameter was not supplied (treated as
    // an empty list).
    uint8_t sv_explain_semantic_view_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        const uint8_t *dims_ptr, size_t dims_len,
        const uint8_t *metrics_ptr, size_t metrics_len,
        const uint8_t *facts_ptr, size_t facts_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);

    // Phase 65 Plan 05 Task 6 (Wave 6) — Rust dispatcher for the bind half
    // of the migrated `semantic_view(view_name, dimensions := [...],
    // metrics := [...], facts := [...])` table function. Same per-call
    // BORROW contract as Wave 5. Performs catalog lookup + expand + LIMIT-0
    // type-inference + execution-SQL construction, and returns:
    //
    //   wire format (u32 little-endian unless noted):
    //     u32 n_cols
    //     for each column:
    //       u32 byte_len + bytes (column name, UTF-8)
    //       u32 duckdb_type_id (normalised: HUGEINT→BIGINT, UHUGEINT→UBIGINT,
    //                           STRUCT/MAP/INVALID→VARCHAR for declaration)
    //     u32 byte_len + bytes (execution_sql, UTF-8)
    //
    // The C++ bind callback parses the buffer, declares the output schema
    // (handling DECIMAL/LIST/ENUM logical-type metadata via a second
    // LIMIT-0 query on the per-call Connection only when needed), stashes
    // the execution_sql in BindData, and runs the actual query inside
    // init_global so chunks can be streamed during exec.
    uint8_t sv_semantic_view_bind_rust(
        duckdb_connection conn,
        const uint8_t *name_ptr, size_t name_len,
        const uint8_t *dims_ptr, size_t dims_len,
        const uint8_t *metrics_ptr, size_t metrics_len,
        const uint8_t *facts_ptr, size_t facts_len,
        char **out_ptr, size_t *out_len,
        char *error_buf, size_t error_buf_len);
}

// ---------------------------------------------------------------------------
// SemanticViewParseData — Phase 62 Plan 03
// ---------------------------------------------------------------------------
// Concrete subclass of ParserExtensionParseData required by the
// ParserExtensionParseResult(unique_ptr<...>) constructor. We never return
// PARSE_SUCCESSFUL from sv_parse_stub (its sole purpose is rendering errors
// via DISPLAY_EXTENSION_ERROR), so this type is structurally needed only
// for layout / type-system reasons. If we ever do produce a parse_data,
// sv_plan_unreachable below would fire.
struct SemanticViewParseData : public ParserExtensionParseData {
    string query;
    explicit SemanticViewParseData(string q) : query(std::move(q)) {}

    unique_ptr<ParserExtensionParseData> Copy() const override {
        return make_uniq<SemanticViewParseData>(query);
    }
    string ToString() const override {
        return query;
    }
};

// RAII guard for heap-owned buffers returned by the Rust FFI. Ensures the
// buffer is released even if a downstream call (Parser::ParseQuery) throws.
struct SvOwnedBuffer {
    char *ptr = nullptr;
    size_t len = 0;
    SvOwnedBuffer() = default;
    SvOwnedBuffer(const SvOwnedBuffer &) = delete;
    SvOwnedBuffer &operator=(const SvOwnedBuffer &) = delete;
    SvOwnedBuffer(SvOwnedBuffer &&other) noexcept
        : ptr(other.ptr), len(other.len) {
        other.ptr = nullptr;
        other.len = 0;
    }
    SvOwnedBuffer &operator=(SvOwnedBuffer &&other) noexcept {
        if (this != &other) {
            if (ptr) sv_free_buffer(ptr, len);
            ptr = other.ptr;
            len = other.len;
            other.ptr = nullptr;
            other.len = 0;
        }
        return *this;
    }
    ~SvOwnedBuffer() {
        if (ptr) sv_free_buffer(ptr, len);
    }
    string to_string() const {
        return ptr ? string(ptr, len) : string();
    }
};

// Per-extension-load marker attached to ParserExtension::parser_info. AR-7:
// this used to carry an opaque Box<OverrideContext>* (rust_state), but the
// OverrideContext was empty after Phase 65 Plan 06's H1 catalog_conn
// retirement, so it now holds no state. It is kept purely as the
// `dynamic_cast<SemanticViewsParserInfo *>` marker type that `sv_parser_override`
// uses to confirm the parser_info is ours. (`sv_parse_stub` no longer consults
// parser_info at all — validation needs no per-DB state — so it ignores the
// marker.)
struct SemanticViewsParserInfo : public ParserExtensionInfo {};

// ---------------------------------------------------------------------------
// Parser-override hook: sv_parser_override
// ---------------------------------------------------------------------------
// The sole DDL entry point. Runs *before* the default parser. Recognized
// semantic-view DDL is rewritten into native SQL by the Rust side and
// re-parsed via DuckDB's own Parser, producing SQLStatement ASTs that DuckDB
// then plans and executes on the caller's connection — so write-side DDL
// participates in the caller's transaction.
//
// For non-matching queries returns DISPLAY_ORIGINAL_ERROR so DuckDB falls
// through to the default parser.
static ParserOverrideResult sv_parser_override(
    ParserExtensionInfo *info, const string &query, ParserOptions &) {

    // Confirm this is our parser_info. info is the per-extension-load
    // SemanticViewsParserInfo attached at registration time; if it isn't ours
    // (dynamic_cast fails) defer to the default parser. AR-7: there is no
    // longer any per-DB Rust state to route through.
    auto *sv_info = dynamic_cast<SemanticViewsParserInfo *>(info);
    if (!sv_info) {
        return ParserOverrideResult();
    }

    SvOwnedBuffer sql_buf;
    char error_buf[1024];
    memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_parser_override_rust(
        query.c_str(), query.size(),
        &sql_buf.ptr, &sql_buf.len,
        error_buf, sizeof(error_buf));

    if (rc == 2) {
        // Not our query — let DuckDB's default parser handle it.
        return ParserOverrideResult();
    }

    if (rc == 1) {
        // Phase 62 Plan 03: defer to default parser. parse_function
        // (registered as sv_parse_stub) re-runs validation and returns
        // DISPLAY_EXTENSION_ERROR with caret position. The Rust side
        // currently always returns rc=2 on the error path, so this
        // branch is defensive — kept to match the documented contract.
        return ParserOverrideResult();
    }

    // rc == 0: native SQL produced. Re-parse via DuckDB's Parser. Use
    // default-constructed ParserOptions so parser_override doesn't recurse
    // (DEFAULT_OVERRIDE skips parser_override hooks entirely). The
    // rewritten SQL is read by exact length, so size is unbounded.
    string native_sql = sql_buf.to_string();
    try {
        Parser parser;
        parser.ParseQuery(native_sql);
        return ParserOverrideResult(std::move(parser.statements));
    } catch (std::exception &e) {
        return ParserOverrideResult(e);
    }
}

// ---------------------------------------------------------------------------
// Parse-function hook: sv_parse_stub  (Phase 62 Plan 03)
// ---------------------------------------------------------------------------
// Called by DuckDB's Parser::ParseQuery after the default parser fails on
// an unrecognised prefix. Sole purpose is rendering caret-aware errors via
// ParserException::SyntaxError(query, msg, error_location).
//
// parser_override remains the success path — it rewrites recognized DDL
// to native SQL, re-parses on the caller's connection, and returns
// PARSE_SUCCESSFUL (transactional behaviour preserved). For error cases
// parser_override now ALWAYS returns DISPLAY_ORIGINAL_ERROR (rc=2 from
// Rust), letting the default parser fail and DuckDB call this stub.
//
// We re-run validation through sv_parse_function_rust which returns:
//   0 — defensive internal error; render as DISPLAY_EXTENSION_ERROR
//   1 — ours-but-invalid: error_buf populated; position is byte offset
//   2 — not ours: DISPLAY_ORIGINAL_ERROR (let the default parser's error stand)
//   3 — valid-but-override-disabled: actionable hint; position=0
static ParserExtensionParseResult sv_parse_stub(
    ParserExtensionInfo * /*info*/, const string &query) {
    // AR-7: sv_parse_function_rust no longer takes a context pointer — the
    // validation path never needed the catalog — so the SemanticViewsParserInfo
    // marker is not consulted here.
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint32_t position = UINT32_MAX;

    uint8_t rc = sv_parse_function_rust(
        query.c_str(), query.size(),
        error_buf, sizeof(error_buf),
        &position);

    switch (rc) {
        case 2:
            // Not ours — DISPLAY_ORIGINAL_ERROR (default parser's error
            // text + caret stands).
            return ParserExtensionParseResult();

        case 1:
        case 3: {
            // ours-but-invalid OR valid-but-override-disabled. Both want
            // DISPLAY_EXTENSION_ERROR with caret position, just different
            // message text. Construct the std::string explicitly first to
            // dodge C++'s "most vexing parse" — `ParserExtensionParseResult
            // result(string(error_buf));` would otherwise be parsed as a
            // function declaration.
            string msg(error_buf);
            ParserExtensionParseResult result(std::move(msg));
            if (position != UINT32_MAX) {
                result.error_location = optional_idx(position);
            }
            return result;
        }

        case 0: {
            // Defensive — sv_parse_function_rust currently never returns 0.
            // Map to DISPLAY_EXTENSION_ERROR with an internal-error message
            // rather than letting a silent default-parser error escape.
            return ParserExtensionParseResult(string(
                "semantic_views: internal error — sv_parse_function_rust "
                "returned rc=0 (please report this bug)"));
        }

        default: {
            // Defensive — unknown rc. Same handling as rc=0.
            return ParserExtensionParseResult(string(
                "semantic_views: internal error — unknown rc from "
                "sv_parse_function_rust"));
        }
    }
}

// ---------------------------------------------------------------------------
// Plan-function hook: sv_plan_unreachable  (Phase 62 Plan 03)
// ---------------------------------------------------------------------------
// ParserExtension carries a sibling `plan_function` pointer alongside
// `parse_function`. The plan function only fires when parse_function returns
// PARSE_SUCCESSFUL, which sv_parse_stub never does — every code path goes
// through DISPLAY_EXTENSION_ERROR or DISPLAY_ORIGINAL_ERROR. Provide a
// hard-fail stub so a contract violation surfaces loudly rather than silently.
//
// Signature must match plan_function_t in parser_extension_compat.hpp:
//   ParserExtensionPlanResult (*)(ParserExtensionInfo *, ClientContext &,
//                                  unique_ptr<ParserExtensionParseData>);
static ParserExtensionPlanResult sv_plan_unreachable(
    ParserExtensionInfo * /*info*/,
    ClientContext & /*context*/,
    unique_ptr<ParserExtensionParseData> /*parse_data*/) {
    throw InternalException(
        "semantic_views: sv_plan_unreachable called — sv_parse_stub never "
        "returns PARSE_SUCCESSFUL (please report this bug)");
}

// ---------------------------------------------------------------------------
// sv_register_table_function — Phase 65 Plan 04 (A2 resolution)
// ---------------------------------------------------------------------------
// Reusable C-callable wrapper around the C++ Catalog API table-function
// registration pattern proven by `65-READ-PATH-SPIKE.md`. Bind callbacks
// registered via this path receive a native `ClientContext &` (not the
// duckdb-rs `BindInfo` wrapper which marshals `ClientContext` away), so
// they can open per-call `Connection(*context.db)` for catalog reads and
// YAML parsing without needing a long-lived extension-owned connection.
//
// Plan 04 consumes this to register `__sv_compute_create_from_yaml`. Plan 05
// will consume it (and add a scalar sibling) to migrate the 17 read-side
// callbacks off `query_conn` (H2).
//
// Header declaration in cpp/src/shim.hpp.
// Phase 65.1 Plan 02a (WR-02 D-08/D-09 + CR-02 D-05) — registration
// failures surface via the `(error_buf, error_buf_len)` trailing pair,
// the same ABI-stable channel used by `sv_parser_override_rust`
// (shim.cpp:57-61). Per D-09 there is NO stderr write — ADBC/JDBC/Python
// callers may have redirected stderr; `error_buf` is the only reliable
// path. D-05: `init_cb == nullptr` is rejected at registration time
// (forces every TF onto the single-shot-via-local-state path so the
// double-emit / unbounded-loop hazard in CR-02 cannot recur).
extern "C" {
    bool sv_register_table_function(
        duckdb_database db_handle,
        const char *name,
        const LogicalType *arg_types,
        size_t arg_count,
        table_function_bind_t bind_cb,
        table_function_t exec_cb,
        table_function_init_local_t init_cb,
        char *error_buf, size_t error_buf_len) {
        // D-05 enforced shape: every required callback (including
        // init_cb — tightened from "may be null" pre-Phase-65.1) is
        // checked at the top before any allocation. Guard the snprintf
        // calls against a null/zero-cap buffer.
        auto write_err = [error_buf, error_buf_len](const char *fmt,
                                                    const char *arg1) {
            if (error_buf == nullptr || error_buf_len == 0) {
                return;
            }
            snprintf(error_buf, error_buf_len, fmt, arg1 ? arg1 : "(null)");
        };
        try {
            if (db_handle == nullptr || name == nullptr ||
                bind_cb == nullptr || exec_cb == nullptr ||
                init_cb == nullptr) {
                write_err(
                    "sv_register_table_function('%s'): null required "
                    "argument (init_cb is mandatory)",
                    name);
                return false;
            }
            // Phase 65.1 WR-01: symmetric null guard for arg_types when the
            // caller declares a non-zero arg_count. The header at
            // cpp/src/shim.hpp documents arg_types "may be null when
            // arg_count == 0"; without this check a buggy caller passing
            // (nullptr, > 0) would segfault inside the loop below — far
            // from the bug site. Matches the D-05/D-08/D-09 hardening
            // pattern already applied to the other input arguments.
            if (arg_count > 0 && arg_types == nullptr) {
                write_err(
                    "sv_register_table_function('%s'): arg_count > 0 but "
                    "arg_types is null",
                    name);
                return false;
            }
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            if (wrapper == nullptr) {
                write_err(
                    "sv_register_table_function('%s'): null DatabaseWrapper",
                    name);
                return false;
            }
            auto &db = *wrapper->database->instance;

            vector<LogicalType> args;
            args.reserve(arg_count);
            for (size_t i = 0; i < arg_count; ++i) {
                args.push_back(arg_types[i]);
            }

            // Six-arg TableFunction ctor: (name, args, function, bind,
            // init_global, init_local). init_global is nullptr; init_cb
            // (init_local) is now mandatory per D-05 — refused above.
            TableFunction tf(
                std::string(name),
                std::move(args),
                exec_cb,
                bind_cb,
                /*init_global*/ nullptr,
                init_cb);

            CreateTableFunctionInfo info(tf);
            // ALTER_ON_CONFLICT: extension reload (LOAD semantic_views after
            // a previous LOAD in the same process) replaces the registration
            // cleanly instead of throwing on duplicate name.
            info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;

            auto &system_catalog = Catalog::GetSystemCatalog(db);
            auto txn = CatalogTransaction::GetSystemTransaction(db);
            system_catalog.CreateTableFunction(txn, info);
            return true;
        } catch (const std::exception &e) {
            if (error_buf != nullptr && error_buf_len > 0) {
                snprintf(error_buf, error_buf_len,
                    "sv_register_table_function('%s') failed: %s",
                    name ? name : "(null)", e.what());
            }
            return false;
        } catch (...) {
            write_err(
                "sv_register_table_function('%s') failed: unknown C++ exception",
                name);
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// sv_register_scalar_function — Phase 65 Plan 05 (Task 2 Step A)
// ---------------------------------------------------------------------------
// Sibling of `sv_register_table_function`. Registers a scalar function via
// the C++ Catalog API (`Catalog::CreateFunction` on the system catalog with
// a `CreateScalarFunctionInfo`). Consumed by Task 4 / Wave 4 to migrate
// `get_ddl` and `read_yaml_from_semantic_view` off
// `register_scalar_function_with_state`.
//
// Header declaration in cpp/src/shim.hpp.
// Phase 65.1 Plan 02a (WR-02 D-08/D-09) — see sv_register_table_function
// for the rationale. Scalar functions have no `init_local` concept, so
// D-05 (null-init refusal) does NOT apply here; the required-arg set
// stays db_handle / name / exec_cb.
extern "C" {
    bool sv_register_scalar_function(
        duckdb_database db_handle,
        const char *name,
        const LogicalType *arg_types,
        size_t arg_count,
        LogicalType return_type,
        scalar_function_t exec_cb,
        char *error_buf, size_t error_buf_len) {
        auto write_err = [error_buf, error_buf_len](const char *fmt,
                                                    const char *arg1) {
            if (error_buf == nullptr || error_buf_len == 0) {
                return;
            }
            snprintf(error_buf, error_buf_len, fmt, arg1 ? arg1 : "(null)");
        };
        try {
            if (db_handle == nullptr || name == nullptr || exec_cb == nullptr) {
                write_err(
                    "sv_register_scalar_function('%s'): null required argument",
                    name);
                return false;
            }
            // Phase 65.1 WR-01: symmetric null guard for arg_types when the
            // caller declares a non-zero arg_count. Same contract as the
            // table-function sibling above.
            if (arg_count > 0 && arg_types == nullptr) {
                write_err(
                    "sv_register_scalar_function('%s'): arg_count > 0 but "
                    "arg_types is null",
                    name);
                return false;
            }
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            if (wrapper == nullptr) {
                write_err(
                    "sv_register_scalar_function('%s'): null DatabaseWrapper",
                    name);
                return false;
            }
            auto &db = *wrapper->database->instance;

            vector<LogicalType> args;
            args.reserve(arg_count);
            for (size_t i = 0; i < arg_count; ++i) {
                args.push_back(arg_types[i]);
            }

            ScalarFunction fn(std::string(name), std::move(args),
                              return_type, exec_cb);
            CreateScalarFunctionInfo info(std::move(fn));
            info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;

            auto &system_catalog = Catalog::GetSystemCatalog(db);
            auto txn = CatalogTransaction::GetSystemTransaction(db);
            system_catalog.CreateFunction(txn, info);
            return true;
        } catch (const std::exception &e) {
            if (error_buf != nullptr && error_buf_len > 0) {
                snprintf(error_buf, error_buf_len,
                    "sv_register_scalar_function('%s') failed: %s",
                    name ? name : "(null)", e.what());
            }
            return false;
        } catch (...) {
            write_err(
                "sv_register_scalar_function('%s') failed: unknown C++ exception",
                name);
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// __sv_compute_create_from_yaml — Phase 65 Plan 04 (Task 2 Step B) +
//                                  Phase 65.1 Plan 07 (CR-01 D-01..D-03)
// ---------------------------------------------------------------------------
// Helper table function registered via `sv_register_table_function`. The bind
// callback reads the YAML file directly through DuckDB's `FileSystem` API
// (`FileSystem::GetFileSystem(context).OpenFile(path, FileFlags::FILE_FLAGS_READ)`
// → `GetFileSize` → `Read`). It then calls the Rust FFI helper
// `sv_compute_create_from_yaml_rust` to parse + enrich + serialize the YAML
// into a metadata-less JSON definition. The exec callback emits the JSON
// as a single VARCHAR row.
//
// Phase 65.1 D-01..D-03 (CR-01): the previous shape used
// `Connection probe(*context.db)` + `Query("SELECT content FROM
// read_text('<path>')")`, which required SQL-quote-doubling on the path. That
// surface is gone: the file is now read directly via the `FileSystem` API.
// `LocalFileSystem` continues to honour the global `enable_external_access`
// DBConfig option when no `FileOpener` is supplied, so the access gate is
// preserved by construction. The four `BinderException` paths below all
// retain the `"FROM YAML FILE failed: "` prefix pinned by
// `test/sql/phase53_yaml_file.test`.
//
// The outer parser_override INSERT (src/parse.rs::rewrite_yaml_file_create,
// landed in Task 4) wraps the helper TF's row via `json_merge_patch(new_def,
// json_object('created_on', strftime(now(), '%Y-%m-%dT%H:%M:%SZ'),
// 'database_name', current_database(), 'schema_name', current_schema()))`
// so metadata fields are populated on the caller's connection, preserving
// the D-21 transactional contract.

struct CreateFromYamlBindData : public TableFunctionData {
    std::string file_path;
    std::string view_name;
    std::string comment;
    std::string new_def;
};

struct CreateFromYamlLocalState : public LocalTableFunctionState {
    bool emitted = false;
};

static unique_ptr<FunctionData> sv_create_from_yaml_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<CreateFromYamlBindData>();
    bd->file_path = input.inputs[0].GetValue<string>();
    bd->view_name = input.inputs[1].GetValue<string>();
    bd->comment   = input.inputs[2].GetValue<string>();

    // Phase 65.1 D-01..D-03 (CR-01): read the YAML file directly via the
    // `FileSystem` API rather than `Connection::Query("SELECT content FROM
    // read_text('<path>')")`. The SQL surface — including its quote-doubling
    // escape — is gone; `LocalFileSystem` honours `enable_external_access`
    // natively when no `FileOpener` is supplied, so the access gate is
    // preserved by construction. Every throw below keeps the
    // `"FROM YAML FILE failed: "` prefix pinned by
    // `test/sql/phase53_yaml_file.test`.
    auto &fs = FileSystem::GetFileSystem(context);
    unique_ptr<FileHandle> handle;
    try {
        handle = fs.OpenFile(bd->file_path, FileFlags::FILE_FLAGS_READ);
    } catch (const std::exception &e) {
        throw BinderException(
            std::string("FROM YAML FILE failed: ") + e.what());
    }
    int64_t size = fs.GetFileSize(*handle);
    if (size < 0) {
        throw BinderException(
            "FROM YAML FILE failed: GetFileSize returned -1 for '" +
            bd->file_path + "'");
    }
    // Phase 65.1 IN-04: fail-fast on hostile file sizes BEFORE allocating
    // `std::string(size, '\0')`. The Rust side enforces a 1 MiB cap via
    // `from_yaml_with_size_cap`, but without a C++ pre-check a multi-GB
    // YAML path would allocate that many bytes of std::string storage,
    // run the read, hand the buffer to Rust, and only then get rejected
    // — practical outcome on a memory-constrained host is OOM kill. The
    // C++ cap is intentionally LOOSER than the Rust cap (16 MiB vs
    // 1 MiB) so the Rust limit remains the single source of truth for
    // legitimate sizing decisions; this cap exists purely as
    // defence-in-depth against a hostile or accidentally-pathological
    // file size.
    constexpr int64_t MAX_YAML_BYTES = 16 * 1024 * 1024;
    if (size > MAX_YAML_BYTES) {
        throw BinderException(
            "FROM YAML FILE failed: file '" + bd->file_path + "' is " +
            std::to_string(size) + " bytes (max " +
            std::to_string(MAX_YAML_BYTES) + ")");
    }
    std::string yaml_content(static_cast<size_t>(size), '\0');
    int64_t got = (size == 0) ? 0
                              : fs.Read(*handle, yaml_content.data(), size);
    if (got != size) {
        throw BinderException(
            "FROM YAML FILE failed: short read (" + std::to_string(got) +
            " of " + std::to_string(size) + ") for '" + bd->file_path + "'");
    }

    // Bridge into Rust to parse + enrich + serialize. The Rust side enforces
    // YAML_SIZE_CAP (1 MiB) inside from_yaml_with_size_cap, which remains the
    // single source of truth for the legitimate size limit. The 16 MiB
    // pre-check above is defence-in-depth against a hostile file size that
    // would balloon the std::string allocation before Rust gets a chance to
    // reject it.
    char *out_ptr = nullptr;
    size_t out_len = 0;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_compute_create_from_yaml_rust(
        reinterpret_cast<const uint8_t *>(yaml_content.data()),
        yaml_content.size(),
        reinterpret_cast<const uint8_t *>(bd->view_name.data()),
        bd->view_name.size(),
        reinterpret_cast<const uint8_t *>(bd->comment.data()),
        bd->comment.size(),
        &out_ptr,
        &out_len,
        error_buf,
        sizeof(error_buf));

    if (rc != 0 || out_ptr == nullptr) {
        // Match "FROM YAML FILE failed:" wording for size/parse errors so
        // the legacy phase53 sqllogictest assertions stay green.
        throw BinderException(
            std::string("FROM YAML FILE failed: ") + error_buf);
    }

    // Move the heap-owned UTF-8 buffer into bd->new_def, then release the
    // Rust allocation via sv_free_buffer (the exact (ptr, len) pair Rust
    // returned). The string copy is unavoidable because std::string demands
    // its own allocator-owned storage; for a 1 MiB cap this is bounded.
    bd->new_def.assign(out_ptr, out_len);
    sv_free_buffer(out_ptr, out_len);

    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("new_def");
    return std::move(bd);
}

static unique_ptr<LocalTableFunctionState> sv_create_from_yaml_init_local(
    ExecutionContext & /*context*/,
    TableFunctionInitInput & /*input*/,
    GlobalTableFunctionState * /*global_state*/) {
    return make_uniq<CreateFromYamlLocalState>();
}

// Phase 65.1 D-07 (IN-01 refresh): this TF's single-shot emission is enforced
// via `init_local` — `sv_create_from_yaml_init_local` (defined just above)
// constructs the `CreateFromYamlLocalState` flag that the exec callback below
// reads to decide whether to emit. `sv_register_table_function` refuses null
// `init_cb` at registration time per Phase 65.1 D-05 (registration helper
// surfaces the refusal text "init_cb is mandatory" via the error_buf), so
// reaching the `InternalException` branch in the exec callback is a
// corrupted-state diagnostic rather than a normal failure mode. The earlier
// recommendation that previously lived here (to rewire registration through
// the init_local callback) is retired — D-05 enforcement (Plan 02) and D-04
// fallback removal (Plan 08) closed it.
static void sv_create_from_yaml_function(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<CreateFromYamlBindData>();
    auto *state_p = data_p.local_state.get();
    if (state_p == nullptr) {
        // Registration refuses null init_cb at registration time per
        // Phase 65.1 D-05 (see `sv_register_table_function` in this file).
        // Reaching this branch indicates a corrupted TableFunctionInput;
        // fail loud rather than risk an unbounded row stream (Phase 65.1
        // D-04, CR-02).
        throw InternalException(
            "sv_create_from_yaml_function: local_state missing despite init_local registration");
    }
    auto &state = state_p->Cast<CreateFromYamlLocalState>();
    if (state.emitted) {
        output.SetCardinality(0);
        return;
    }
    output.SetValue(0, 0, Value(bd.new_def));
    output.SetCardinality(1);
    state.emitted = true;
}

// ---------------------------------------------------------------------------
// list_semantic_views — Phase 65 Plan 05 (Task 1 / Wave 0 bridge spike)
// ---------------------------------------------------------------------------
// First read-side TF migrated from duckdb-rs `register_table_function_with_
// extra_info` (which marshals `ClientContext &` away — Plan 01 Spike A6) to
// the C++ Catalog API path via `sv_register_table_function` (Plan 04). The
// bind callback opens a per-call `Connection probe(*context.db)` and bridges
// to Rust by casting the stack `Connection *` to `duckdb_connection`. See
// the FFI declaration block above + `65-05-SPIKE-SUMMARY.md` for the bridge
// mechanism design and the LOC extrapolation for the remaining 16 read-side
// migrations.
//
// Wire format from Rust dispatcher: a flat length-prefixed binary buffer:
//   u32 row_count (little-endian)
//   for each row:
//     for each of 6 columns:
//       u32 byte_len (little-endian)
//       byte_len bytes (UTF-8, NOT NUL-terminated)
// Buffer is heap-owned by Rust; C++ parses into BindData and immediately
// releases via `sv_free_buffer`.

struct ListSemanticViewsRow {
    std::string created_on;
    std::string name;
    std::string kind;
    std::string database_name;
    std::string schema_name;
    std::string comment;
};

struct ListSemanticViewsBindData : public TableFunctionData {
    std::vector<ListSemanticViewsRow> rows;
};

struct ListSemanticViewsLocalState : public LocalTableFunctionState {
    // Next row to emit. Bind-materialized row sets can exceed one
    // DataChunk's capacity (STANDARD_VECTOR_SIZE = 2048), so the exec
    // callback must emit in chunks and resume from this cursor — a
    // single-shot `emitted` flag overflowed the chunk for >2048 rows
    // (writes past the vector's data buffer; no bounds check in
    // release builds).
    idx_t next_row = 0;
};

// Helper: read a little-endian u32 from buf[offset..offset+4] and advance.
// Throws BinderException on out-of-bounds — defensive against an FFI buffer
// truncated by a panic or allocation failure on the Rust side.
static uint32_t sv_read_u32_le(const char *buf, size_t buf_len, size_t &offset) {
    if (offset + 4 > buf_len) {
        throw BinderException(
            "list_semantic_views: FFI buffer truncated (expected u32 at offset " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
    auto p = reinterpret_cast<const unsigned char *>(buf + offset);
    uint32_t v = static_cast<uint32_t>(p[0])
               | (static_cast<uint32_t>(p[1]) << 8)
               | (static_cast<uint32_t>(p[2]) << 16)
               | (static_cast<uint32_t>(p[3]) << 24);
    offset += 4;
    return v;
}

static std::string sv_read_string(const char *buf, size_t buf_len, size_t &offset) {
    uint32_t len = sv_read_u32_le(buf, buf_len, offset);
    if (offset + len > buf_len) {
        throw BinderException(
            "list_semantic_views: FFI buffer truncated (expected " +
            std::to_string(len) + " string bytes at offset " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
    std::string s(buf + offset, len);
    offset += len;
    return s;
}

static unique_ptr<FunctionData> sv_list_semantic_views_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<ListSemanticViewsBindData>();

    // Declare the 6-column schema — must match the v0.9.0 Rust VTab exactly
    // (test/sql/phase42_persistence.test + any other suite that does
    // SELECT * FROM list_semantic_views() relies on byte-identical column
    // names and order).
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("created_on");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("name");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("kind");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("database_name");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("schema_name");
    return_types.push_back(LogicalType::VARCHAR);
    names.push_back("comment");

    // Open a per-call Connection on the caller's DatabaseInstance. The
    // ctor calls `ConnectionManager::AddConnection` (acquires
    // `connections_lock`) and the matching dtor (RemoveConnection)
    // releases it at end-of-scope. Both confirmed deadlock-free from the
    // bind thread by `65-READ-PATH-SPIKE.md` (READ-BIND-RC0).
    Connection probe(*context.db);

    // Bridge: cast the stack Connection* to duckdb_connection. The C API
    // handle is literally `reinterpret_cast<duckdb_connection>(Connection*)`
    // — confirmed by reading the amalgamation:
    //
    //   // duckdb.cpp:266440-266446
    //   connection = new Connection(*wrapper->database);
    //   *out = reinterpret_cast<duckdb_connection>(connection);
    //
    // This is a BORROW: ownership stays with the C++ `probe` local. The
    // Rust dispatcher MUST NOT call `duckdb_disconnect` on the handle (it
    // would `delete` a stack-allocated Connection — UB). The bind scope's
    // destructor handles teardown correctly.
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_list_semantic_views_bind_rust(
        borrowed,
        &payload.ptr, &payload.len,
        error_buf, sizeof(error_buf));

    if (rc != 0) {
        // Surface as BinderException so the user sees a proper SQL-layer
        // error rather than a process crash. The Rust dispatcher's
        // `catch_unwind` already converts panics into rc=2 + an error
        // message; this branch just re-raises across the C++ boundary.
        throw BinderException(
            std::string("list_semantic_views failed: ") + error_buf);
    }

    if (payload.ptr == nullptr) {
        // Defensive: rc=0 with null buffer means zero rows — bd->rows
        // stays empty. Skip parsing.
        return std::move(bd);
    }

    // Parse the flat binary buffer into ListSemanticViewsRow entries.
    size_t offset = 0;
    uint32_t row_count = sv_read_u32_le(payload.ptr, payload.len, offset);
    bd->rows.reserve(row_count);
    for (uint32_t r = 0; r < row_count; ++r) {
        ListSemanticViewsRow row;
        row.created_on    = sv_read_string(payload.ptr, payload.len, offset);
        row.name          = sv_read_string(payload.ptr, payload.len, offset);
        row.kind          = sv_read_string(payload.ptr, payload.len, offset);
        row.database_name = sv_read_string(payload.ptr, payload.len, offset);
        row.schema_name   = sv_read_string(payload.ptr, payload.len, offset);
        row.comment       = sv_read_string(payload.ptr, payload.len, offset);
        bd->rows.push_back(std::move(row));
    }

    // SvOwnedBuffer destructor releases the Rust-owned buffer via
    // sv_free_buffer with the exact (ptr, len) pair.
    return std::move(bd);
}

static unique_ptr<LocalTableFunctionState> sv_list_semantic_views_init_local(
    ExecutionContext & /*context*/,
    TableFunctionInitInput & /*input*/,
    GlobalTableFunctionState * /*global_state*/) {
    return make_uniq<ListSemanticViewsLocalState>();
}

static void sv_list_semantic_views_function(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<ListSemanticViewsBindData>();
    auto *state_p = data_p.local_state.get();
    if (state_p == nullptr) {
        // Registration refuses null init_cb at registration time per
        // Phase 65.1 D-05 (see `sv_register_table_function` in this file).
        // Reaching this branch indicates a corrupted TableFunctionInput;
        // fail loud rather than risk an unbounded row stream (Phase 65.1
        // D-04, CR-02).
        throw InternalException(
            "sv_list_semantic_views_function: local_state missing despite init_local registration");
    }
    auto &state = state_p->Cast<ListSemanticViewsLocalState>();
    idx_t total = bd.rows.size();
    if (state.next_row >= total) {
        output.SetCardinality(0);
        return;
    }
    idx_t remaining = total - state.next_row;
    idx_t count = remaining < STANDARD_VECTOR_SIZE ? remaining : STANDARD_VECTOR_SIZE;
    for (idx_t i = 0; i < count; ++i) {
        const auto &row = bd.rows[state.next_row + i];
        output.SetValue(0, i, Value(row.created_on));
        output.SetValue(1, i, Value(row.name));
        output.SetValue(2, i, Value(row.kind));
        output.SetValue(3, i, Value(row.database_name));
        output.SetValue(4, i, Value(row.schema_name));
        output.SetValue(5, i, Value(row.comment));
    }
    output.SetCardinality(count);
    state.next_row += count;
}

extern "C" {
    bool sv_register_list_semantic_views(duckdb_database db_handle,
                                         char *error_buf, size_t error_buf_len) {
        // Zero-argument table function — no arg_types array.
        return sv_register_table_function(
            db_handle,
            "list_semantic_views",
            /*arg_types*/ nullptr, /*arg_count*/ 0,
            sv_list_semantic_views_bind,
            sv_list_semantic_views_function,
            sv_list_semantic_views_init_local,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 (Tasks 2-5) — Generic VARCHAR-rows + VARCHAR/BOOL-rows
// helpers for the remaining read-side TF migrations.
// ---------------------------------------------------------------------------
//
// Most of the Phase 65 Plan 05 migrations emit homogeneous VARCHAR result
// columns (the various SHOW SEMANTIC ... / DESCRIBE SEMANTIC VIEW shapes).
// To keep the per-TF migration delta to a thin adapter, we factor the
// payload-parse + bind-data shape + exec emitter into generic helpers here.
//
// Each migrated TF still owns its own bind callback (to declare the right
// column names and call the matching `sv_<name>_bind_rust` dispatcher), but
// the bulk of the bind + the entire exec/init are reused.

struct SvVarcharBindData : public TableFunctionData {
    std::vector<std::vector<std::string>> rows;
    size_t expected_cols = 0;
};

struct SvVarcharLocalState : public LocalTableFunctionState {
    // Next row to emit — see ListSemanticViewsLocalState::next_row for why
    // this is a cursor and not a single-shot flag (chunked emission past
    // STANDARD_VECTOR_SIZE rows). Shared by sv_emit_varchar_rows and
    // sv_emit_varchar_bool_rows.
    idx_t next_row = 0;
};

// Parse a length-prefixed VARCHAR wire-format payload into bd.rows. The
// expected column count is taken from bd.expected_cols (must be set by the
// caller before invocation). Throws BinderException on truncated payload.
static void sv_parse_varchar_payload(const char *buf, size_t buf_len,
                                     SvVarcharBindData &bd,
                                     const char *fn_name) {
    if (buf == nullptr) {
        return;  // rc=0 with null buffer == zero rows
    }
    size_t offset = 0;
    uint32_t row_count = sv_read_u32_le(buf, buf_len, offset);
    bd.rows.reserve(row_count);
    for (uint32_t r = 0; r < row_count; ++r) {
        std::vector<std::string> row;
        row.reserve(bd.expected_cols);
        for (size_t c = 0; c < bd.expected_cols; ++c) {
            row.push_back(sv_read_string(buf, buf_len, offset));
        }
        bd.rows.push_back(std::move(row));
    }
    if (offset != buf_len) {
        throw BinderException(
            std::string(fn_name) +
            ": FFI buffer has trailing bytes (consumed " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
}

static unique_ptr<LocalTableFunctionState> sv_varchar_init_local(
    ExecutionContext & /*context*/,
    TableFunctionInitInput & /*input*/,
    GlobalTableFunctionState * /*global_state*/) {
    return make_uniq<SvVarcharLocalState>();
}

static void sv_emit_varchar_rows(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<SvVarcharBindData>();
    auto *state_p = data_p.local_state.get();
    if (state_p == nullptr) {
        // Registration refuses null init_cb at registration time per
        // Phase 65.1 D-05 (see `sv_register_table_function` in this file).
        // Reaching this branch indicates a corrupted TableFunctionInput;
        // fail loud rather than risk an unbounded row stream (Phase 65.1
        // D-04, CR-02).
        throw InternalException(
            "sv_emit_varchar_rows: local_state missing despite init_local registration");
    }
    auto &state = state_p->Cast<SvVarcharLocalState>();
    idx_t total = bd.rows.size();
    if (state.next_row >= total) {
        output.SetCardinality(0);
        return;
    }
    idx_t remaining = total - state.next_row;
    idx_t count = remaining < STANDARD_VECTOR_SIZE ? remaining : STANDARD_VECTOR_SIZE;
    for (idx_t i = 0; i < count; ++i) {
        const auto &row = bd.rows[state.next_row + i];
        for (size_t c = 0; c < row.size(); ++c) {
            output.SetValue(c, i, Value(row[c]));
        }
    }
    output.SetCardinality(count);
    state.next_row += count;
}

// VARCHAR-rows-with-trailing-BOOL shape (used by
// show_semantic_dimensions_for_metric, which returns 3 VARCHAR + 1 BOOLEAN).
struct SvVarcharBoolBindData : public TableFunctionData {
    std::vector<std::pair<std::vector<std::string>, bool>> rows;
    size_t expected_varchar_cols = 0;  // number of VARCHAR cells per row
};

static void sv_parse_varchar_bool_payload(const char *buf, size_t buf_len,
                                          SvVarcharBoolBindData &bd,
                                          const char *fn_name) {
    if (buf == nullptr) {
        return;
    }
    size_t offset = 0;
    uint32_t row_count = sv_read_u32_le(buf, buf_len, offset);
    bd.rows.reserve(row_count);
    for (uint32_t r = 0; r < row_count; ++r) {
        std::vector<std::string> strs;
        strs.reserve(bd.expected_varchar_cols);
        for (size_t c = 0; c < bd.expected_varchar_cols; ++c) {
            strs.push_back(sv_read_string(buf, buf_len, offset));
        }
        if (offset + 1 > buf_len) {
            throw BinderException(
                std::string(fn_name) +
                ": FFI buffer truncated (expected u8 bool at offset " +
                std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
        }
        bool b = buf[offset] != 0;
        offset += 1;
        bd.rows.emplace_back(std::move(strs), b);
    }
    if (offset != buf_len) {
        throw BinderException(
            std::string(fn_name) +
            ": FFI buffer has trailing bytes (consumed " +
            std::to_string(offset) + " of " + std::to_string(buf_len) + ")");
    }
}

static void sv_emit_varchar_bool_rows(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<SvVarcharBoolBindData>();
    auto *state_p = data_p.local_state.get();
    if (state_p == nullptr) {
        // Registration refuses null init_cb at registration time per
        // Phase 65.1 D-05 (see `sv_register_table_function` in this file).
        // Reaching this branch indicates a corrupted TableFunctionInput;
        // fail loud rather than risk an unbounded row stream (Phase 65.1
        // D-04, CR-02).
        throw InternalException(
            "sv_emit_varchar_bool_rows: local_state missing despite init_local registration");
    }
    auto &state = state_p->Cast<SvVarcharLocalState>();
    idx_t total = bd.rows.size();
    if (state.next_row >= total) {
        output.SetCardinality(0);
        return;
    }
    idx_t remaining = total - state.next_row;
    idx_t count = remaining < STANDARD_VECTOR_SIZE ? remaining : STANDARD_VECTOR_SIZE;
    for (idx_t i = 0; i < count; ++i) {
        const auto &strs = bd.rows[state.next_row + i].first;
        for (size_t c = 0; c < strs.size(); ++c) {
            output.SetValue(c, i, Value(strs[c]));
        }
        // BOOLEAN trailing column at index strs.size().
        output.SetValue(strs.size(), i, Value::BOOLEAN(bd.rows[state.next_row + i].second));
    }
    output.SetCardinality(count);
    state.next_row += count;
}

// Common idiom for the bind callback: open Connection, run Rust dispatcher
// for the zero-arg case, parse payload into bd. Used by all 5 zero-arg
// migrated TFs (Task 2 / Wave 1) and reused by Task 3 / Wave 2 with extra
// VARCHAR args supplied to the dispatcher.
//
// `dispatcher` returns rc==0 on success (payload populated), rc!=0 with
// error_buf NUL-terminated otherwise. Throws BinderException on rc!=0 with
// the "<fn_name> failed: <error_buf>" message — matches the Wave 0 spike.
template <typename DispatcherFn>
static void sv_run_varchar_bind(ClientContext &context,
                                SvVarcharBindData &bd,
                                size_t expected_cols,
                                const char *fn_name,
                                DispatcherFn &&dispatcher) {
    bd.expected_cols = expected_cols;
    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = dispatcher(borrowed,
                            &payload.ptr, &payload.len,
                            error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException(std::string(fn_name) + " failed: " + error_buf);
    }
    sv_parse_varchar_payload(payload.ptr, payload.len, bd, fn_name);
}

// Variant for TFs with one VARCHAR argument (the view name). Extracts the
// name from input.inputs[0] and forwards it as (ptr, len) to the dispatcher
// alongside the borrowed connection handle.
template <typename DispatcherFn>
static void sv_run_varchar_bind_with_name(ClientContext &context,
                                          TableFunctionBindInput &input,
                                          SvVarcharBindData &bd,
                                          size_t expected_cols,
                                          const char *fn_name,
                                          DispatcherFn &&dispatcher) {
    bd.expected_cols = expected_cols;
    // FF-4: guard the positional view-name argument. Without this a NULL
    // argument (e.g. describe_semantic_view(NULL)) reached GetValue<string>()
    // and surfaced as the confusing "view 'NULL' does not exist"; match the
    // semantic_view() binder's up-front BinderException instead.
    if (input.inputs.empty() || input.inputs[0].IsNull()) {
        throw BinderException(std::string(fn_name) +
                              ": view name is required (positional arg 0)");
    }
    std::string name = input.inputs[0].GetValue<std::string>();
    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = dispatcher(borrowed,
                            reinterpret_cast<const uint8_t *>(name.data()), name.size(),
                            &payload.ptr, &payload.len,
                            error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException(std::string(fn_name) + " failed: " + error_buf);
    }
    sv_parse_varchar_payload(payload.ptr, payload.len, bd, fn_name);
}

// Variant for TFs with two VARCHAR arguments (view_name, metric_name).
template <typename DispatcherFn>
static void sv_run_varchar_bool_bind_with_two_names(
    ClientContext &context,
    TableFunctionBindInput &input,
    SvVarcharBoolBindData &bd,
    size_t expected_varchar_cols,
    const char *fn_name,
    DispatcherFn &&dispatcher) {
    bd.expected_varchar_cols = expected_varchar_cols;
    // FF-4: guard both positional arguments (view_name, metric_name) before
    // GetValue<string>() so a NULL argument raises a clear BinderException
    // rather than resolving to a spurious "'NULL' does not exist".
    if (input.inputs.size() < 2 || input.inputs[0].IsNull() ||
        input.inputs[1].IsNull()) {
        throw BinderException(
            std::string(fn_name) +
            ": view name and metric name are required (positional args 0, 1)");
    }
    std::string name1 = input.inputs[0].GetValue<std::string>();
    std::string name2 = input.inputs[1].GetValue<std::string>();
    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = dispatcher(borrowed,
                            reinterpret_cast<const uint8_t *>(name1.data()), name1.size(),
                            reinterpret_cast<const uint8_t *>(name2.data()), name2.size(),
                            &payload.ptr, &payload.len,
                            error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException(std::string(fn_name) + " failed: " + error_buf);
    }
    sv_parse_varchar_bool_payload(payload.ptr, payload.len, bd, fn_name);
}

template <typename DispatcherFn>
static void sv_run_varchar_bool_bind(ClientContext &context,
                                     SvVarcharBoolBindData &bd,
                                     size_t expected_varchar_cols,
                                     const char *fn_name,
                                     DispatcherFn &&dispatcher) {
    bd.expected_varchar_cols = expected_varchar_cols;
    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);
    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = dispatcher(borrowed,
                            &payload.ptr, &payload.len,
                            error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException(std::string(fn_name) + " failed: " + error_buf);
    }
    sv_parse_varchar_bool_payload(payload.ptr, payload.len, bd, fn_name);
}

// ---------------------------------------------------------------------------
// list_terse_semantic_views — Phase 65 Plan 05 Task 2 (Wave 1)
// ---------------------------------------------------------------------------
// 5-column subset of list_semantic_views — same bridge mechanism, no
// per-function BindData struct (uses generic SvVarcharBindData via the
// sv_run_varchar_bind helper).
//
// Columns: created_on, name, kind, database_name, schema_name.

static unique_ptr<FunctionData> sv_list_terse_semantic_views_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COL_NAMES[] = {
        "created_on", "name", "kind", "database_name", "schema_name",
    };
    for (auto cn : COL_NAMES) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind(
        context, *bd, /*expected_cols*/ 5, "list_terse_semantic_views",
        [](duckdb_connection borrowed, char **out_ptr, size_t *out_len,
           char *error_buf, size_t error_buf_len) {
            return sv_list_terse_semantic_views_bind_rust(
                borrowed, out_ptr, out_len, error_buf, error_buf_len);
        });
    return std::move(bd);
}

extern "C" {
    bool sv_register_list_terse_semantic_views(duckdb_database db_handle,
                                               char *error_buf, size_t error_buf_len) {
        return sv_register_table_function(
            db_handle,
            "list_terse_semantic_views",
            /*arg_types*/ nullptr, /*arg_count*/ 0,
            sv_list_terse_semantic_views_bind,
            sv_emit_varchar_rows,
            sv_varchar_init_local,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// show_semantic_dimensions_all — Phase 65 Plan 05 Task 2 (Wave 1)
// ---------------------------------------------------------------------------
// 8-column VARCHAR: database_name, schema_name, semantic_view_name,
// table_name, name, data_type, synonyms, comment.

static unique_ptr<FunctionData> sv_show_semantic_dimensions_all_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name", "table_name",
        "name", "data_type", "synonyms", "comment",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind(
        context, *bd, /*expected_cols*/ 8, "show_semantic_dimensions_all",
        [](duckdb_connection borrowed, char **out_ptr, size_t *out_len,
           char *error_buf, size_t error_buf_len) {
            return sv_show_semantic_dimensions_all_bind_rust(
                borrowed, out_ptr, out_len, error_buf, error_buf_len);
        });
    return std::move(bd);
}

extern "C" {
    bool sv_register_show_semantic_dimensions_all(duckdb_database db_handle,
                                                  char *error_buf, size_t error_buf_len) {
        return sv_register_table_function(
            db_handle, "show_semantic_dimensions_all",
            nullptr, 0,
            sv_show_semantic_dimensions_all_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// show_semantic_metrics_all — Phase 65 Plan 05 Task 2 (Wave 1)
// ---------------------------------------------------------------------------
// 8-column VARCHAR (same schema as show_semantic_dimensions_all).

static unique_ptr<FunctionData> sv_show_semantic_metrics_all_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name", "table_name",
        "name", "data_type", "synonyms", "comment",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind(
        context, *bd, 8, "show_semantic_metrics_all",
        [](duckdb_connection borrowed, char **out_ptr, size_t *out_len,
           char *error_buf, size_t error_buf_len) {
            return sv_show_semantic_metrics_all_bind_rust(
                borrowed, out_ptr, out_len, error_buf, error_buf_len);
        });
    return std::move(bd);
}

extern "C" {
    bool sv_register_show_semantic_metrics_all(duckdb_database db_handle,
                                               char *error_buf, size_t error_buf_len) {
        return sv_register_table_function(
            db_handle, "show_semantic_metrics_all",
            nullptr, 0,
            sv_show_semantic_metrics_all_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// show_semantic_facts_all — Phase 65 Plan 05 Task 2 (Wave 1)
// ---------------------------------------------------------------------------
// 8-column VARCHAR (same schema as show_semantic_dimensions_all).

static unique_ptr<FunctionData> sv_show_semantic_facts_all_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name", "table_name",
        "name", "data_type", "synonyms", "comment",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind(
        context, *bd, 8, "show_semantic_facts_all",
        [](duckdb_connection borrowed, char **out_ptr, size_t *out_len,
           char *error_buf, size_t error_buf_len) {
            return sv_show_semantic_facts_all_bind_rust(
                borrowed, out_ptr, out_len, error_buf, error_buf_len);
        });
    return std::move(bd);
}

extern "C" {
    bool sv_register_show_semantic_facts_all(duckdb_database db_handle,
                                             char *error_buf, size_t error_buf_len) {
        return sv_register_table_function(
            db_handle, "show_semantic_facts_all",
            nullptr, 0,
            sv_show_semantic_facts_all_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// show_semantic_materializations_all — Phase 65 Plan 05 Task 2 (Wave 1)
// ---------------------------------------------------------------------------
// 7-column VARCHAR: database_name, schema_name, semantic_view_name, name,
// table, dimensions, metrics.

static unique_ptr<FunctionData> sv_show_semantic_materializations_all_bind(
    ClientContext &context,
    TableFunctionBindInput & /*input*/,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name",
        "name", "table", "dimensions", "metrics",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind(
        context, *bd, 7, "show_semantic_materializations_all",
        [](duckdb_connection borrowed, char **out_ptr, size_t *out_len,
           char *error_buf, size_t error_buf_len) {
            return sv_show_semantic_materializations_all_bind_rust(
                borrowed, out_ptr, out_len, error_buf, error_buf_len);
        });
    return std::move(bd);
}

extern "C" {
    bool sv_register_show_semantic_materializations_all(duckdb_database db_handle,
                                                        char *error_buf, size_t error_buf_len) {
        return sv_register_table_function(
            db_handle, "show_semantic_materializations_all",
            nullptr, 0,
            sv_show_semantic_materializations_all_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// Wave 2 single-arg TFs — Phase 65 Plan 05 Task 3
// ---------------------------------------------------------------------------
// show_columns_in_semantic_view, describe_semantic_view, and the 4
// single-view show_semantic_<entity> variants all take one VARCHAR arg
// (the view name) and emit homogeneous VARCHAR rows. The single
// show_semantic_dimensions_for_metric takes two VARCHAR args and emits
// VARCHAR + trailing BOOL rows (handled in the dedicated block further
// down).

static unique_ptr<FunctionData> sv_show_columns_in_semantic_view_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name", "column_name",
        "data_type", "kind", "expression", "comment",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind_with_name(
        context, input, *bd, 8, "show_columns_in_semantic_view",
        [](duckdb_connection borrowed,
           const uint8_t *np, size_t nl,
           char **op, size_t *ol, char *eb, size_t ebl) {
            return sv_show_columns_in_semantic_view_bind_rust(
                borrowed, np, nl, op, ol, eb, ebl);
        });
    return std::move(bd);
}

static unique_ptr<FunctionData> sv_describe_semantic_view_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "object_kind", "object_name", "parent_entity", "property", "property_value",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind_with_name(
        context, input, *bd, 5, "describe_semantic_view",
        [](duckdb_connection borrowed,
           const uint8_t *np, size_t nl,
           char **op, size_t *ol, char *eb, size_t ebl) {
            return sv_describe_semantic_view_bind_rust(
                borrowed, np, nl, op, ol, eb, ebl);
        });
    return std::move(bd);
}

static unique_ptr<FunctionData> sv_show_semantic_dimensions_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name", "table_name",
        "name", "data_type", "synonyms", "comment",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind_with_name(
        context, input, *bd, 8, "show_semantic_dimensions",
        [](duckdb_connection borrowed,
           const uint8_t *np, size_t nl,
           char **op, size_t *ol, char *eb, size_t ebl) {
            return sv_show_semantic_dimensions_bind_rust(
                borrowed, np, nl, op, ol, eb, ebl);
        });
    return std::move(bd);
}

static unique_ptr<FunctionData> sv_show_semantic_metrics_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name", "table_name",
        "name", "data_type", "synonyms", "comment",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind_with_name(
        context, input, *bd, 8, "show_semantic_metrics",
        [](duckdb_connection borrowed,
           const uint8_t *np, size_t nl,
           char **op, size_t *ol, char *eb, size_t ebl) {
            return sv_show_semantic_metrics_bind_rust(
                borrowed, np, nl, op, ol, eb, ebl);
        });
    return std::move(bd);
}

static unique_ptr<FunctionData> sv_show_semantic_facts_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name", "table_name",
        "name", "data_type", "synonyms", "comment",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind_with_name(
        context, input, *bd, 8, "show_semantic_facts",
        [](duckdb_connection borrowed,
           const uint8_t *np, size_t nl,
           char **op, size_t *ol, char *eb, size_t ebl) {
            return sv_show_semantic_facts_bind_rust(
                borrowed, np, nl, op, ol, eb, ebl);
        });
    return std::move(bd);
}

static unique_ptr<FunctionData> sv_show_semantic_materializations_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    static const char *const COLS[] = {
        "database_name", "schema_name", "semantic_view_name",
        "name", "table", "dimensions", "metrics",
    };
    for (auto cn : COLS) {
        return_types.push_back(LogicalType::VARCHAR);
        names.emplace_back(cn);
    }
    sv_run_varchar_bind_with_name(
        context, input, *bd, 7, "show_semantic_materializations",
        [](duckdb_connection borrowed,
           const uint8_t *np, size_t nl,
           char **op, size_t *ol, char *eb, size_t ebl) {
            return sv_show_semantic_materializations_bind_rust(
                borrowed, np, nl, op, ol, eb, ebl);
        });
    return std::move(bd);
}

// show_semantic_dimensions_for_metric: 3 VARCHAR + 1 BOOL output, 2 VARCHAR
// input args.
static unique_ptr<FunctionData> sv_show_semantic_dimensions_for_metric_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBoolBindData>();
    return_types.push_back(LogicalType::VARCHAR);
    names.emplace_back("table_name");
    return_types.push_back(LogicalType::VARCHAR);
    names.emplace_back("name");
    return_types.push_back(LogicalType::VARCHAR);
    names.emplace_back("data_type");
    return_types.push_back(LogicalType::BOOLEAN);
    names.emplace_back("required");
    sv_run_varchar_bool_bind_with_two_names(
        context, input, *bd, /*expected_varchar_cols*/ 3,
        "show_semantic_dimensions_for_metric",
        [](duckdb_connection borrowed,
           const uint8_t *vn, size_t vnl,
           const uint8_t *mn, size_t mnl,
           char **op, size_t *ol, char *eb, size_t ebl) {
            return sv_show_semantic_dimensions_for_metric_bind_rust(
                borrowed, vn, vnl, mn, mnl, op, ol, eb, ebl);
        });
    return std::move(bd);
}

extern "C" {
    bool sv_register_show_columns_in_semantic_view(duckdb_database db_handle,
                                                   char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR};
        return sv_register_table_function(
            db_handle, "show_columns_in_semantic_view",
            args, 1,
            sv_show_columns_in_semantic_view_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
    bool sv_register_describe_semantic_view(duckdb_database db_handle,
                                            char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR};
        return sv_register_table_function(
            db_handle, "describe_semantic_view",
            args, 1,
            sv_describe_semantic_view_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
    bool sv_register_show_semantic_dimensions(duckdb_database db_handle,
                                              char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR};
        return sv_register_table_function(
            db_handle, "show_semantic_dimensions",
            args, 1,
            sv_show_semantic_dimensions_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
    bool sv_register_show_semantic_metrics(duckdb_database db_handle,
                                           char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR};
        return sv_register_table_function(
            db_handle, "show_semantic_metrics",
            args, 1,
            sv_show_semantic_metrics_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
    bool sv_register_show_semantic_facts(duckdb_database db_handle,
                                         char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR};
        return sv_register_table_function(
            db_handle, "show_semantic_facts",
            args, 1,
            sv_show_semantic_facts_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
    bool sv_register_show_semantic_materializations(duckdb_database db_handle,
                                                    char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR};
        return sv_register_table_function(
            db_handle, "show_semantic_materializations",
            args, 1,
            sv_show_semantic_materializations_bind,
            sv_emit_varchar_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
    bool sv_register_show_semantic_dimensions_for_metric(duckdb_database db_handle,
                                                         char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR, LogicalType::VARCHAR};
        return sv_register_table_function(
            db_handle, "show_semantic_dimensions_for_metric",
            args, 2,
            sv_show_semantic_dimensions_for_metric_bind,
            sv_emit_varchar_bool_rows, sv_varchar_init_local,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 4 (Wave 3) — scalar function migrations
// ---------------------------------------------------------------------------
//
// Migrate the two read-side scalars (`get_ddl`, `read_yaml_from_semantic_view`)
// from duckdb-rs `register_scalar_function_with_state` to the C++ Catalog
// API path established by `sv_register_scalar_function` (Wave 1 prep). Each
// exec callback opens a per-call `Connection probe(*state.GetContext().db)`
// (the scalar analog of the bind-side `Connection(*context.db)` used by the
// 15 TF migrations) and bridges to a Rust dispatcher that performs the
// lookup + render on the borrowed connection. The borrow contract (Rust
// MUST NOT call `duckdb_disconnect`) is identical to the TF dispatchers —
// see `src/ddl/read_ffi.rs` module docs.
//
// Per-chunk Connection construction (one `Connection probe` per exec call,
// reused across every row in the chunk) keeps the dispatcher shape uniform
// with the TF migrations: each call into the C++ exec callback gets a
// fresh stack-owned Connection bound to the caller's Database, and every
// row in the chunk borrows that same handle through the reinterpret_cast
// bridge. Scalar usage in practice is one or two rows
// (`SELECT GET_DDL(...)`), so amortising the Connection ctor across the
// chunk is essentially free even at chunk size 1 — and the ctor itself is
// sub-millisecond per the READ-PATH-SPIKE evidence.

// Common helper: copy a Rust-owned UTF-8 buffer into the result Vector at
// `row_idx` via StringVector::AddString (which allocates inside the Vector's
// string heap). Throws InvalidInputException on rc!=0 with the dispatcher's
// error_buf as the message. The owned buffer is always freed via
// `sv_free_buffer` regardless of rc.
template <typename DispatcherFn>
static void sv_emit_scalar_row(Vector &result, idx_t row_idx,
                               const char *fn_name,
                               DispatcherFn &&dispatcher) {
    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = dispatcher(&payload.ptr, &payload.len,
                            error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw InvalidInputException(std::string(fn_name) + ": " + error_buf);
    }
    // payload.ptr is non-null on rc==0 (publish_owned_buffer guarantees this
    // when out_ptr is non-null); len may be 0 for an empty string.
    string_t out_value = StringVector::AddString(
        result,
        payload.ptr == nullptr ? "" : payload.ptr,
        payload.len);
    FlatVector::GetData<string_t>(result)[row_idx] = out_value;
}

// get_ddl(object_type VARCHAR, name VARCHAR) -> VARCHAR
static void sv_get_ddl_exec(DataChunk &args, ExpressionState &state,
                            Vector &result) {
    auto &type_vec = args.data[0];
    auto &name_vec = args.data[1];
    type_vec.Flatten(args.size());
    name_vec.Flatten(args.size());
    auto type_data = FlatVector::GetData<string_t>(type_vec);
    auto name_data = FlatVector::GetData<string_t>(name_vec);
    auto &type_validity = FlatVector::Validity(type_vec);
    auto &name_validity = FlatVector::Validity(name_vec);

    auto &result_validity = FlatVector::Validity(result);

    Connection probe(*state.GetContext().db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

    for (idx_t i = 0; i < args.size(); ++i) {
        if (!type_validity.RowIsValid(i) || !name_validity.RowIsValid(i)) {
            result_validity.SetInvalid(i);
            continue;
        }
        const string_t &t = type_data[i];
        const string_t &n = name_data[i];
        sv_emit_scalar_row(
            result, i, "get_ddl",
            [&](char **op, size_t *ol, char *eb, size_t ebl) {
                return sv_get_ddl_exec_rust(
                    borrowed,
                    reinterpret_cast<const uint8_t *>(t.GetData()), t.GetSize(),
                    reinterpret_cast<const uint8_t *>(n.GetData()), n.GetSize(),
                    op, ol, eb, ebl);
            });
    }
    if (args.AllConstant()) {
        result.SetVectorType(VectorType::CONSTANT_VECTOR);
    }
}

// read_yaml_from_semantic_view(name VARCHAR) -> VARCHAR
static void sv_read_yaml_from_semantic_view_exec(DataChunk &args,
                                                 ExpressionState &state,
                                                 Vector &result) {
    auto &name_vec = args.data[0];
    name_vec.Flatten(args.size());
    auto name_data = FlatVector::GetData<string_t>(name_vec);
    auto &name_validity = FlatVector::Validity(name_vec);
    auto &result_validity = FlatVector::Validity(result);

    Connection probe(*state.GetContext().db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

    for (idx_t i = 0; i < args.size(); ++i) {
        if (!name_validity.RowIsValid(i)) {
            result_validity.SetInvalid(i);
            continue;
        }
        const string_t &n = name_data[i];
        sv_emit_scalar_row(
            result, i, "read_yaml_from_semantic_view",
            [&](char **op, size_t *ol, char *eb, size_t ebl) {
                return sv_read_yaml_from_semantic_view_exec_rust(
                    borrowed,
                    reinterpret_cast<const uint8_t *>(n.GetData()), n.GetSize(),
                    op, ol, eb, ebl);
            });
    }
    if (args.AllConstant()) {
        result.SetVectorType(VectorType::CONSTANT_VECTOR);
    }
}

extern "C" {
    bool sv_register_get_ddl(duckdb_database db_handle,
                             char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR, LogicalType::VARCHAR};
        return sv_register_scalar_function(
            db_handle, "get_ddl",
            args, 2,
            LogicalType::VARCHAR,
            sv_get_ddl_exec,
            error_buf, error_buf_len);
    }
    bool sv_register_read_yaml_from_semantic_view(duckdb_database db_handle,
                                                  char *error_buf, size_t error_buf_len) {
        LogicalType args[] = {LogicalType::VARCHAR};
        return sv_register_scalar_function(
            db_handle, "read_yaml_from_semantic_view",
            args, 1,
            LogicalType::VARCHAR,
            sv_read_yaml_from_semantic_view_exec,
            error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 5 (Wave 5) — explain_semantic_view migration
// ---------------------------------------------------------------------------
//
// Migrates `explain_semantic_view(view_name, dimensions := [...],
// metrics := [...], facts := [...])` off duckdb-rs's
// `register_table_function_with_extra_info` (which marshals
// `ClientContext &` away) to the C++ Catalog API path. The bind callback
// opens a per-call `Connection probe(*context.db)` and bridges to the
// Rust dispatcher `sv_explain_semantic_view_bind_rust` which runs the
// catalog lookup + expand + EXPLAIN on the per-call connection. Output is
// one VARCHAR row per explain-output line — reuses the Wave 1/2
// `SvVarcharBindData` shape + `sv_emit_varchar_rows` exec.
//
// Named LIST(VARCHAR) parameter handling: the three optional named
// parameters are flattened on the C++ side using
// `sv_serialise_string_list` (length-prefixed wire format) and passed as
// (ptr, len) pairs to the Rust dispatcher. Missing named parameters are
// passed as nullptr+0, which the Rust side treats as an empty list.

// Serialise a LIST(VARCHAR) Value into the standard length-prefixed wire
// format (`u32 count; for each: u32 byte_len + bytes`). The returned bytes
// can be handed directly to the Rust dispatcher as (ptr, len). Throws
// BinderException if the Value is not a LIST or contains non-VARCHAR
// children — defensive: DuckDB's named-parameter type-check already
// enforces the LIST(VARCHAR) declaration at registration time, so a
// mismatch here is a planner bug.
static std::vector<uint8_t> sv_serialise_string_list(
    const Value &list_val, const char *param_name) {
    std::vector<uint8_t> buf;
    const auto &children = ListValue::GetChildren(list_val);
    uint32_t count = static_cast<uint32_t>(children.size());
    buf.reserve(4 + children.size() * 16);
    buf.push_back(static_cast<uint8_t>(count & 0xff));
    buf.push_back(static_cast<uint8_t>((count >> 8) & 0xff));
    buf.push_back(static_cast<uint8_t>((count >> 16) & 0xff));
    buf.push_back(static_cast<uint8_t>((count >> 24) & 0xff));
    for (const auto &c : children) {
        if (c.IsNull()) {
            throw BinderException(
                std::string("explain_semantic_view: `") + param_name +
                "` contains a NULL element (only non-NULL VARCHARs accepted)");
        }
        // c.GetValue<std::string>() applies any necessary cast. The named-
        // parameter declaration is LIST(VARCHAR) so this should be a no-op.
        std::string s = c.GetValue<std::string>();
        uint32_t len = static_cast<uint32_t>(s.size());
        buf.push_back(static_cast<uint8_t>(len & 0xff));
        buf.push_back(static_cast<uint8_t>((len >> 8) & 0xff));
        buf.push_back(static_cast<uint8_t>((len >> 16) & 0xff));
        buf.push_back(static_cast<uint8_t>((len >> 24) & 0xff));
        buf.insert(buf.end(), s.begin(), s.end());
    }
    return buf;
}

static unique_ptr<FunctionData> sv_explain_semantic_view_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    auto bd = make_uniq<SvVarcharBindData>();
    bd->expected_cols = 1;
    return_types.push_back(LogicalType::VARCHAR);
    names.emplace_back("explain_output");

    if (input.inputs.empty() || input.inputs[0].IsNull()) {
        throw BinderException(
            "explain_semantic_view: view name is required (positional arg 0)");
    }
    std::string view_name = input.inputs[0].GetValue<std::string>();

    // Pull the three optional named LIST(VARCHAR) parameters. The
    // `input.named_parameters` map is case-insensitive (per
    // case_insensitive_map_t). A missing entry means the user did not
    // supply that named parameter — pass nullptr+0 to the Rust side.
    std::vector<uint8_t> dims_buf, metrics_buf, facts_buf;
    auto it_d = input.named_parameters.find("dimensions");
    if (it_d != input.named_parameters.end() && !it_d->second.IsNull()) {
        dims_buf = sv_serialise_string_list(it_d->second, "dimensions");
    }
    auto it_m = input.named_parameters.find("metrics");
    if (it_m != input.named_parameters.end() && !it_m->second.IsNull()) {
        metrics_buf = sv_serialise_string_list(it_m->second, "metrics");
    }
    auto it_f = input.named_parameters.find("facts");
    if (it_f != input.named_parameters.end() && !it_f->second.IsNull()) {
        facts_buf = sv_serialise_string_list(it_f->second, "facts");
    }

    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));

    uint8_t rc = sv_explain_semantic_view_bind_rust(
        borrowed,
        reinterpret_cast<const uint8_t *>(view_name.data()), view_name.size(),
        dims_buf.empty()    ? nullptr : dims_buf.data(),    dims_buf.size(),
        metrics_buf.empty() ? nullptr : metrics_buf.data(), metrics_buf.size(),
        facts_buf.empty()   ? nullptr : facts_buf.data(),   facts_buf.size(),
        &payload.ptr, &payload.len,
        error_buf, sizeof(error_buf));

    if (rc != 0) {
        throw BinderException(std::string("explain_semantic_view: ") + error_buf);
    }
    sv_parse_varchar_payload(payload.ptr, payload.len, *bd,
                             "explain_semantic_view");
    return std::move(bd);
}

// sv_register_table_function does not declare named parameters; for
// explain_semantic_view (and Wave 6's semantic_view) we need them so
// DuckDB's binder type-checks `dimensions := [...]` etc. against
// LIST(VARCHAR). Build the TableFunction by hand and register it via the
// same Catalog::CreateTableFunction path.
//
// Borrow contract identical to sv_register_table_function: callbacks
// receive `ClientContext &` and the bind opens per-call Connections —
// no long-lived extension-owned `duckdb_connection`.
// Phase 65.1 Plan 02a (WR-02 D-08/D-09) — hand-built impl writes
// failures directly into the supplied `error_buf` via snprintf instead
// of stderr, mirroring `sv_register_table_function`. The buffer
// argument is forwarded by `sv_register_explain_semantic_view` below;
// init_extension (Plan 02b) supplies it.
static bool sv_register_explain_semantic_view_impl(duckdb_database db_handle,
                                                   char *error_buf,
                                                   size_t error_buf_len) {
    auto write_err = [error_buf, error_buf_len](const char *msg) {
        if (error_buf == nullptr || error_buf_len == 0) {
            return;
        }
        snprintf(error_buf, error_buf_len, "%s", msg);
    };
    try {
        auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
            db_handle->internal_ptr);
        if (wrapper == nullptr) {
            write_err(
                "sv_register_explain_semantic_view: null DatabaseWrapper");
            return false;
        }
        auto &db = *wrapper->database->instance;

        vector<LogicalType> args = {LogicalType::VARCHAR};
        TableFunction tf(
            std::string("explain_semantic_view"),
            std::move(args),
            sv_emit_varchar_rows,
            sv_explain_semantic_view_bind,
            /*init_global*/ nullptr,
            sv_varchar_init_local);

        // Named LIST(VARCHAR) parameters — match the legacy Rust VTab
        // signature (`dimensions`, `metrics`, `facts`) byte-for-byte so
        // existing call sites continue to parse without surprises.
        tf.named_parameters["dimensions"] = LogicalType::LIST(LogicalType::VARCHAR);
        tf.named_parameters["metrics"]    = LogicalType::LIST(LogicalType::VARCHAR);
        tf.named_parameters["facts"]      = LogicalType::LIST(LogicalType::VARCHAR);

        CreateTableFunctionInfo info(tf);
        info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;

        auto &system_catalog = Catalog::GetSystemCatalog(db);
        auto txn = CatalogTransaction::GetSystemTransaction(db);
        system_catalog.CreateTableFunction(txn, info);
        return true;
    } catch (const std::exception &e) {
        if (error_buf != nullptr && error_buf_len > 0) {
            snprintf(error_buf, error_buf_len,
                "sv_register_explain_semantic_view failed: %s", e.what());
        }
        return false;
    } catch (...) {
        write_err(
            "sv_register_explain_semantic_view failed: unknown C++ exception");
        return false;
    }
}

extern "C" {
    bool sv_register_explain_semantic_view(duckdb_database db_handle,
                                           char *error_buf, size_t error_buf_len) {
        return sv_register_explain_semantic_view_impl(
            db_handle, error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// Phase 65 Plan 05 Task 6 (Wave 6) — semantic_view migration
// ---------------------------------------------------------------------------
//
// Migrates the main expansion-path `semantic_view(view_name, dimensions :=
// [...], metrics := [...], facts := [...])` table function off duckdb-rs's
// `register_table_function_with_extra_info` (which marshals
// `ClientContext &` away and consumed the long-lived H2 query_conn) to
// the C++ Catalog API path. The bind callback opens a per-call
// `Connection probe(*context.db)` and dispatches to
// `sv_semantic_view_bind_rust` for catalog lookup + expand + LIMIT-0 type
// inference + execution-SQL construction.
//
// Streaming model: bind returns a column schema + the execution SQL
// string in BindData. `init_global` opens its OWN per-call
// `Connection probe(*context.db)`, runs `probe.Query(execution_sql)` to
// materialise the result, and stashes the
// `unique_ptr<MaterializedQueryResult>` inside the GlobalState. The
// Connection drops at end of init_global; the materialised result is
// self-contained (`ColumnDataCollection` owns its storage) and lives
// for the rest of the query. `func()` then fetches chunks via
// `result->Fetch()` and copies vector data into the output DataChunk.
//
// Per-call lifecycle: TWO Connections are constructed per
// `semantic_view(...)` invocation — one for bind (catalog probe +
// LIMIT 0 type inference), one for init_global (actual query). Both
// drop before any exec call runs. No long-lived extension-owned
// `duckdb_connection` is consumed by this path.
//
// Borrow contract: identical to the 15 prior migrations. The Rust
// dispatcher MUST NOT call `duckdb_disconnect` on the borrowed handle —
// teardown is the C++ scope's responsibility.

struct SemanticViewColumnInfo {
    std::string name;
    uint32_t type_id = 0;  // C-API DUCKDB_TYPE_* enum value (normalised:
                           // HUGEINT→BIGINT, UHUGEINT→UBIGINT).
};

// Map a C-API `DUCKDB_TYPE` enum value (which is what
// `duckdb_column_type` / `ffi::duckdb_column_type` return in Rust) to a
// C++ `LogicalType`. The two enum spaces have DIFFERENT integer values
// (e.g. C-API DUCKDB_TYPE_DECIMAL=19 vs C++ LogicalTypeId::DECIMAL=21),
// so a naive `static_cast<LogicalTypeId>(c_type_id)` would silently
// mis-type columns. This helper is the single source of truth for the
// conversion and mirrors `type_from_duckdb_type_u32` /
// `declare_output_type` in `src/query/table_function.rs`.
//
// DECIMAL / LIST / ENUM intentionally return a default (precisionless)
// LogicalType — callers (see `sv_resolve_output_logical_types`) override
// with the probed LogicalType from the LIMIT-0 result so width/scale/
// child-type are preserved.
static LogicalType sv_logical_type_from_c_type_id(uint32_t c_type_id) {
    using duckdb::LogicalTypeId;
    switch (c_type_id) {
        case DUCKDB_TYPE_INVALID:    return LogicalType::VARCHAR;  // fallback
        case DUCKDB_TYPE_BOOLEAN:    return LogicalType::BOOLEAN;
        case DUCKDB_TYPE_TINYINT:    return LogicalType::TINYINT;
        case DUCKDB_TYPE_SMALLINT:   return LogicalType::SMALLINT;
        case DUCKDB_TYPE_INTEGER:    return LogicalType::INTEGER;
        case DUCKDB_TYPE_BIGINT:     return LogicalType::BIGINT;
        case DUCKDB_TYPE_UTINYINT:   return LogicalType::UTINYINT;
        case DUCKDB_TYPE_USMALLINT:  return LogicalType::USMALLINT;
        case DUCKDB_TYPE_UINTEGER:   return LogicalType::UINTEGER;
        case DUCKDB_TYPE_UBIGINT:    return LogicalType::UBIGINT;
        case DUCKDB_TYPE_FLOAT:      return LogicalType::FLOAT;
        case DUCKDB_TYPE_DOUBLE:     return LogicalType::DOUBLE;
        case DUCKDB_TYPE_TIMESTAMP:  return LogicalType::TIMESTAMP;
        case DUCKDB_TYPE_DATE:       return LogicalType::DATE;
        case DUCKDB_TYPE_TIME:       return LogicalType::TIME;
        case DUCKDB_TYPE_INTERVAL:   return LogicalType::INTERVAL;
        case DUCKDB_TYPE_HUGEINT:    return LogicalType::BIGINT;   // normalised
        case DUCKDB_TYPE_UHUGEINT:   return LogicalType::UBIGINT;  // normalised
        case DUCKDB_TYPE_VARCHAR:    return LogicalType::VARCHAR;
        case DUCKDB_TYPE_BLOB:       return LogicalType::BLOB;
        case DUCKDB_TYPE_TIMESTAMP_S:  return LogicalType::TIMESTAMP_S;
        case DUCKDB_TYPE_TIMESTAMP_MS: return LogicalType::TIMESTAMP_MS;
        case DUCKDB_TYPE_TIMESTAMP_NS: return LogicalType::TIMESTAMP_NS;
        case DUCKDB_TYPE_TIMESTAMP_TZ: return LogicalType::TIMESTAMP_TZ;
        case DUCKDB_TYPE_TIME_TZ:    return LogicalType::TIME_TZ;
        case DUCKDB_TYPE_UUID:       return LogicalType::UUID;
        case DUCKDB_TYPE_BIT:        return LogicalType::BIT;
        case DUCKDB_TYPE_ENUM:       return LogicalType::VARCHAR;  // declared as VARCHAR
        case DUCKDB_TYPE_STRUCT:     return LogicalType::VARCHAR;  // fallback to VARCHAR
        case DUCKDB_TYPE_MAP:        return LogicalType::VARCHAR;
        case DUCKDB_TYPE_DECIMAL:    return LogicalType::DECIMAL(18, 3);  // placeholder
        case DUCKDB_TYPE_LIST:       return LogicalType::LIST(LogicalType::VARCHAR);  // placeholder
        default:
            // Unknown type — declare VARCHAR so downstream operators don't
            // get a junk LogicalTypeId. Type mismatch will surface at exec.
            return LogicalType::VARCHAR;
    }
}

struct SemanticViewBindData : public TableFunctionData {
    std::vector<SemanticViewColumnInfo> columns;
    std::string execution_sql;
    std::string expanded_sql_for_error;  // Mirror of execution_sql for SqlExecution error.
};

struct SemanticViewGlobalState : public GlobalTableFunctionState {
    std::unique_ptr<MaterializedQueryResult> result;
    idx_t emitted_chunks = 0;
};

// Look up DECIMAL/LIST logical types via a LIMIT-0 probe. Returns a
// per-column LogicalType for every column in the declared schema; for
// columns whose type_id isn't DECIMAL/LIST, the LogicalType is constructed
// from the type_id directly (matching the behaviour of
// `type_from_duckdb_type_u32` on the Rust side).
//
// Phase 65.1 WR-03: ENUM is intentionally declared as VARCHAR via
// `sv_logical_type_from_c_type_id` (see that helper) — the LIMIT-0 probe
// is NOT required for ENUM. The earlier comment claiming otherwise was a
// maintenance trap.
//
// Phase 65.1 WR-07: takes the caller's per-bind Connection by reference
// rather than opening a fresh one. This eliminates one ctor/dtor pair
// per `semantic_view(...)` invocation when DECIMAL/LIST is present and,
// more importantly, ensures the bind-time LIMIT-0 query runs on the same
// Connection (and therefore the same transaction/catalog snapshot) as
// the Rust dispatcher's catalog reads. Previously the FFI dispatcher
// borrowed one Connection while this helper opened a second — two
// separate Connections viewing potentially different uncommitted state.
//
// Probe failures (DuckDB error from the LIMIT 0 query) and column-count
// mismatches now raise BinderException with full diagnostic text instead
// of silently falling back to a type_id-only declaration (Phase 65.1
// Plan 11 / WR-07 / D-14). The previous fallback masked real DDL-level
// errors (broken FACTS / METRICS expressions referencing nonexistent
// columns) behind a DECIMAL(18,3) placeholder at query time.
static std::vector<LogicalType> sv_resolve_output_logical_types(
    Connection &probe,
    const std::vector<SemanticViewColumnInfo> &cols,
    const std::string &execution_sql) {
    std::vector<LogicalType> out;
    out.reserve(cols.size());

    bool needs_logical_probe = false;
    for (const auto &c : cols) {
        // C-API DUCKDB_TYPE_* enum values from duckdb.h. These two need
        // logical-type metadata to declare correctly: DECIMAL needs
        // width+scale; LIST needs child type. ENUM is intentionally
        // declared as VARCHAR via sv_logical_type_from_c_type_id, so no
        // probe is needed for it (Phase 65.1 WR-03).
        if (c.type_id == DUCKDB_TYPE_DECIMAL ||
            c.type_id == DUCKDB_TYPE_LIST) {
            needs_logical_probe = true;
            break;
        }
    }

    if (!needs_logical_probe) {
        // Fast path: build each LogicalType straight from the C-API type_id.
        for (const auto &c : cols) {
            out.emplace_back(sv_logical_type_from_c_type_id(c.type_id));
        }
        return out;
    }

    // Slow path: run LIMIT 0 on the caller-supplied Connection to extract
    // logical type metadata for DECIMAL/LIST columns. Reusing the bind's
    // Connection (rather than opening a fresh one) preserves the
    // transaction/catalog snapshot the FFI dispatcher already borrowed.
    std::string limit0_sql = "SELECT * FROM (" + execution_sql + ") __sv_probe LIMIT 0";
    auto probe_result = probe.Query(limit0_sql);
    if (probe_result->HasError()) {
        // Phase 65.1 Plan 11 / WR-07 / D-14: surface the underlying DuckDB
        // error verbatim. No silent fallback to a type_id-only declaration.
        throw BinderException(
            "semantic_view: failed to infer column types via LIMIT 0 probe: " +
            probe_result->GetError());
    }
    if (probe_result->types.size() != cols.size()) {
        // Phase 65.1 Plan 11 / WR-07 / D-14: distinct diagnostic naming
        // both counts so a future contributor can see which side disagrees.
        throw BinderException(
            "semantic_view: LIMIT 0 probe returned " +
            std::to_string(probe_result->types.size()) +
            " columns, expected " + std::to_string(cols.size()));
    }

    for (idx_t i = 0; i < cols.size(); ++i) {
        const auto &c = cols[i];
        const LogicalType &probed = probe_result->types[i];
        if (c.type_id == DUCKDB_TYPE_DECIMAL || c.type_id == DUCKDB_TYPE_LIST) {
            // Use the probed logical type (preserves width/scale/child type).
            out.emplace_back(probed);
        } else {
            out.emplace_back(sv_logical_type_from_c_type_id(c.type_id));
        }
    }
    return out;
}

static unique_ptr<FunctionData> sv_semantic_view_bind(
    ClientContext &context,
    TableFunctionBindInput &input,
    vector<LogicalType> &return_types,
    vector<string> &names) {
    if (input.inputs.empty() || input.inputs[0].IsNull()) {
        throw BinderException(
            "semantic_view: view name is required (positional arg 0)");
    }
    std::string view_name = input.inputs[0].GetValue<std::string>();

    std::vector<uint8_t> dims_buf, metrics_buf, facts_buf;
    auto it_d = input.named_parameters.find("dimensions");
    if (it_d != input.named_parameters.end() && !it_d->second.IsNull()) {
        dims_buf = sv_serialise_string_list(it_d->second, "dimensions");
    }
    auto it_m = input.named_parameters.find("metrics");
    if (it_m != input.named_parameters.end() && !it_m->second.IsNull()) {
        metrics_buf = sv_serialise_string_list(it_m->second, "metrics");
    }
    auto it_f = input.named_parameters.find("facts");
    if (it_f != input.named_parameters.end() && !it_f->second.IsNull()) {
        facts_buf = sv_serialise_string_list(it_f->second, "facts");
    }

    Connection probe(*context.db);
    duckdb_connection borrowed = reinterpret_cast<duckdb_connection>(&probe);

    SvOwnedBuffer payload;
    char error_buf[1024];
    std::memset(error_buf, 0, sizeof(error_buf));
    uint8_t rc = sv_semantic_view_bind_rust(
        borrowed,
        reinterpret_cast<const uint8_t *>(view_name.data()), view_name.size(),
        dims_buf.empty()    ? nullptr : dims_buf.data(),    dims_buf.size(),
        metrics_buf.empty() ? nullptr : metrics_buf.data(), metrics_buf.size(),
        facts_buf.empty()   ? nullptr : facts_buf.data(),   facts_buf.size(),
        &payload.ptr, &payload.len,
        error_buf, sizeof(error_buf));
    if (rc != 0) {
        throw BinderException(std::string("semantic_view: ") + error_buf);
    }

    auto bd = make_uniq<SemanticViewBindData>();

    // Parse the schema + execution_sql wire format.
    size_t offset = 0;
    uint32_t n_cols = sv_read_u32_le(payload.ptr, payload.len, offset);
    bd->columns.reserve(n_cols);
    for (uint32_t i = 0; i < n_cols; ++i) {
        SemanticViewColumnInfo info;
        info.name = sv_read_string(payload.ptr, payload.len, offset);
        info.type_id = sv_read_u32_le(payload.ptr, payload.len, offset);
        bd->columns.push_back(std::move(info));
    }
    bd->execution_sql = sv_read_string(payload.ptr, payload.len, offset);
    if (offset != payload.len) {
        throw BinderException(
            "semantic_view: FFI buffer has trailing bytes (consumed " +
            std::to_string(offset) + " of " + std::to_string(payload.len) + ")");
    }
    bd->expanded_sql_for_error = bd->execution_sql;

    // Resolve declared logical types — runs a LIMIT-0 probe on the SAME
    // Connection the FFI dispatcher already borrowed, if any DECIMAL/LIST
    // column is in the schema (so width/scale/child-type can be honoured).
    // Phase 65.1 WR-07: reusing `probe` here avoids a second Connection
    // ctor/dtor pair and keeps both queries on the same
    // transaction/catalog snapshot.
    auto declared_types = sv_resolve_output_logical_types(
        probe, bd->columns, bd->execution_sql);
    for (idx_t i = 0; i < bd->columns.size(); ++i) {
        return_types.push_back(declared_types[i]);
        names.push_back(bd->columns[i].name);
    }
    return std::move(bd);
}

static unique_ptr<GlobalTableFunctionState> sv_semantic_view_init_global(
    ClientContext &context,
    TableFunctionInitInput &input) {
    auto &bd = input.bind_data->Cast<SemanticViewBindData>();
    auto state = make_uniq<SemanticViewGlobalState>();

    // Open a per-call Connection on the caller's DatabaseInstance and run
    // the materialised query. The Connection drops at end of scope; the
    // returned MaterializedQueryResult owns its data (via ColumnDataCollection)
    // and is safe to use across exec invocations.
    Connection probe(*context.db);
    auto qresult = probe.Query(bd.execution_sql);
    if (qresult->HasError()) {
        // Match the legacy QueryError::SqlExecution wording so existing
        // sqllogictest matchers stay byte-identical.
        throw InvalidInputException(
            "semantic_view: SQL execution failed: " + qresult->GetError() +
            "  (expanded SQL: " + bd.expanded_sql_for_error + ")");
    }
    if (qresult->type != QueryResultType::MATERIALIZED_RESULT) {
        throw InternalException(
            "semantic_view: expected MaterializedQueryResult from Connection::Query");
    }
    state->result = unique_ptr_cast<QueryResult, MaterializedQueryResult>(std::move(qresult));
    return std::move(state);
}

static void sv_semantic_view_function(
    ClientContext & /*context*/,
    TableFunctionInput &data_p,
    DataChunk &output) {
    auto &bd = data_p.bind_data->Cast<SemanticViewBindData>();
    auto &gs = data_p.global_state->Cast<SemanticViewGlobalState>();

    // Fetch the next chunk from the materialised result. Fetch() flattens
    // vectors and returns nullptr when the stream is exhausted.
    auto chunk = gs.result->Fetch();
    if (!chunk || chunk->size() == 0) {
        output.SetCardinality(0);
        return;
    }

    // Validate column count — defensive against a planner inserting projections
    // we didn't anticipate. The legacy Rust impl had a Mutex + col_count check;
    // for the C++ path the bind-time schema agreement is authoritative.
    //
    // Phase 65.1 WR-02: previously this took `std::min(chunk->ColumnCount(),
    // bd.columns.size())` and silently truncated on mismatch, leaving any
    // unfilled output slots with default-constructed Vector contents that
    // are not guaranteed to be NULL-marked — readers downstream could see
    // garbage. Replace with an explicit mismatch error so a future planner
    // change that violates the bind contract fails loud at first emission
    // rather than producing silently-wrong data.
    if (chunk->ColumnCount() != bd.columns.size()) {
        throw InternalException(
            "semantic_view: column count mismatch — chunk has " +
            std::to_string(chunk->ColumnCount()) + " columns, bind declared " +
            std::to_string(bd.columns.size()) +
            " (please report this bug)");
    }
    idx_t n_cols = chunk->ColumnCount();

    // Copy each column into the output chunk via Vector::Reference (zero-copy
    // when types match exactly; falls back to a cast when build_execution_sql
    // already inserted a `::TYPE` wrapper but the runtime type still differs).
    for (idx_t col_idx = 0; col_idx < n_cols; ++col_idx) {
        auto &src = chunk->data[col_idx];
        auto &dst = output.data[col_idx];
        if (src.GetType() == dst.GetType()) {
            // Same logical type — zero-copy reference.
            dst.Reference(src);
        } else {
            // Type mismatch despite the bind-time cast wrapper — last-resort
            // cast. Matches the legacy QueryError::TypeMismatch fallback but
            // does the cast instead of erroring; the bind-time cast wrapper
            // (build_execution_sql) is the primary defence.
            VectorOperations::DefaultCast(src, dst, chunk->size());
        }
    }
    output.SetCardinality(chunk->size());
    gs.emitted_chunks++;
}

// Register semantic_view via the C++ Catalog API — same pattern as
// sv_register_explain_semantic_view_impl. Constructs TableFunction by hand
// (rather than going through sv_register_table_function) so the
// `named_parameters` map can be populated for `dimensions` / `metrics` /
// `facts`. Also passes `init_global` because semantic_view's exec needs
// per-execution global state (the materialised query result).
// Phase 65.1 Plan 02a (WR-02 D-08/D-09) — see
// `sv_register_explain_semantic_view_impl` for the rationale. Same
// snprintf-into-error_buf convention; no stderr writes.
static bool sv_register_semantic_view_impl(duckdb_database db_handle,
                                           char *error_buf,
                                           size_t error_buf_len) {
    auto write_err = [error_buf, error_buf_len](const char *msg) {
        if (error_buf == nullptr || error_buf_len == 0) {
            return;
        }
        snprintf(error_buf, error_buf_len, "%s", msg);
    };
    try {
        auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
            db_handle->internal_ptr);
        if (wrapper == nullptr) {
            write_err(
                "sv_register_semantic_view: null DatabaseWrapper");
            return false;
        }
        auto &db = *wrapper->database->instance;

        vector<LogicalType> args = {LogicalType::VARCHAR};
        TableFunction tf(
            std::string("semantic_view"),
            std::move(args),
            sv_semantic_view_function,
            sv_semantic_view_bind,
            sv_semantic_view_init_global,
            /*init_local*/ nullptr);

        tf.named_parameters["dimensions"] = LogicalType::LIST(LogicalType::VARCHAR);
        tf.named_parameters["metrics"]    = LogicalType::LIST(LogicalType::VARCHAR);
        tf.named_parameters["facts"]      = LogicalType::LIST(LogicalType::VARCHAR);

        CreateTableFunctionInfo info(tf);
        info.on_conflict = OnCreateConflict::ALTER_ON_CONFLICT;

        auto &system_catalog = Catalog::GetSystemCatalog(db);
        auto txn = CatalogTransaction::GetSystemTransaction(db);
        system_catalog.CreateTableFunction(txn, info);
        return true;
    } catch (const std::exception &e) {
        if (error_buf != nullptr && error_buf_len > 0) {
            snprintf(error_buf, error_buf_len,
                "sv_register_semantic_view failed: %s", e.what());
        }
        return false;
    } catch (...) {
        write_err(
            "sv_register_semantic_view failed: unknown C++ exception");
        return false;
    }
}

extern "C" {
    bool sv_register_semantic_view(duckdb_database db_handle,
                                   char *error_buf, size_t error_buf_len) {
        return sv_register_semantic_view_impl(
            db_handle, error_buf, error_buf_len);
    }
}

// ---------------------------------------------------------------------------
// sv_register_parser_hooks -- called from Rust after C API init
// ---------------------------------------------------------------------------
// Extracts DatabaseInstance& from the C API handle and registers the
// parser_override hook on DBConfig. Phase 65 Plan 06: signature slimmed
// to `(db_handle)` after H1 catalog_conn retirement. AR-7: the
// SemanticViewsParserInfo carries no Rust state anymore — it is just the
// dynamic_cast marker used by sv_parser_override / sv_parse_stub to confirm
// the parser_info is ours.
//
// Phase 65.1 Plan 10 (WR-06 D-12/D-13): the signature now carries the
// trailing `(char *error_buf, size_t error_buf_len)` pair — matching the
// 17 read-side dispatchers — so registration failures and BORROW-contract
// probe failures surface through the ABI-stable channel into the Rust
// caller's diagnostic. A runtime probe right after the wrapper unwrap
// catches representation drift the file-scope `static_assert` cannot.
extern "C" {
    bool sv_register_parser_hooks(
        duckdb_database db_handle,
        char *error_buf, size_t error_buf_len) {
        try {
            // duckdb_database -> internal_ptr -> DatabaseWrapper ->
            //   shared_ptr<DuckDB> -> shared_ptr<DatabaseInstance>
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            auto &db = *wrapper->database->instance;

            // Phase 65.1 Plan 10 (WR-06 D-12 runtime probe). The file-scope
            // `static_assert(sizeof(duckdb_connection) == sizeof(void*))`
            // catches size drift; this probe catches representation drift
            // that an equal-size change would miss (e.g. wrapping the
            // pointer in a struct with the same layout but different
            // semantics). Failure refuses the LOAD entirely so no
            // semantic_views state survives a broken bridge — better than
            // discovering the breakage on the first user query.
            //
            // The `Connection probe` is stack-allocated; its dtor runs at
            // scope exit. The bridged `handle` becomes invalid but is not
            // touched after the SELECT 1 round-trip.
            {
                Connection probe(db);
                auto handle = reinterpret_cast<duckdb_connection>(&probe);
                duckdb_result r;
                auto rc = duckdb_query(handle, "SELECT 1", &r);
                if (rc != DuckDBSuccess) {
                    // Phase 65.1 IN-01: surface the underlying DuckDB error
                    // text and clarify that the probe failure can stem from
                    // either ABI drift (the original BORROW-contract concern)
                    // OR a transient DB error at LOAD time (shutdown in
                    // progress, OOM, etc.). The diagnostic must list both
                    // possible causes so an operator hitting a non-bridge
                    // failure isn't misled into chasing an ABI regression.
                    const char *err_text = duckdb_result_error(&r);
                    if (error_buf != nullptr && error_buf_len > 0) {
                        snprintf(error_buf, error_buf_len,
                            "bridge contract probe failed (duckdb_query "
                            "SELECT 1 returned %d): %s. Possible causes: "
                            "duckdb_connection ABI drift OR transient DB "
                            "error at LOAD time; refusing to load "
                            "semantic_views extension.",
                            (int)rc, err_text ? err_text : "(no detail)");
                    }
                    duckdb_destroy_result(&r);
                    return false;
                }
                duckdb_destroy_result(&r);
            }

            auto &config = DBConfig::GetConfig(db);

            // Phase 65.1 Plan 12 (WR-09 D-21): idempotence check across
            // same-process repeat LOAD semantic_views. `ParserExtension::Register`
            // unconditionally appends to `DBConfig::parser_extensions` (no dedup
            // API), so without this guard every LOAD grows the list by one entry
            // pointing at the same `sv_parser_override` — an unbounded "soft leak"
            // surfaced by 65-REVIEW.md WR-09.
            //
            // We iterate the public read-side iterator
            // `DBConfig::GetCallbackManager().ParserExtensions()`
            // (cpp/include/duckdb.cpp:281157) and compare each existing entry's
            // `parser_override` function pointer against the file-static
            // `sv_parser_override` symbol. Function-pointer equality is portable
            // ISO C++ here because `sv_parser_override` is a file-static C++
            // function defined in this same TU — both sides of the `==` see the
            // identical pointer value.
            //
            // CRITICAL: the entire `ParserExtension ext; ...; Register(...)` block
            // — including the `SemanticViewsParserInfo` allocation — must be
            // guarded. `Register` unconditionally appends to
            // `DBConfig::parser_extensions`; on the skip path re-registering
            // would grow the list unboundedly across repeated re-LOAD cycles
            // (the precise leak shape WR-09 / CR-01 (65.1) was meant to fix).
            //
            // The target is same-process repeat LOAD (e.g. a long-lived host
            // re-invoking `LOAD semantic_views` across sessions, or partial-failure
            // retries during `init_extension`). The theoretical concurrent-thread
            // LOAD race between the iterator check and `Register` is documented
            // as out of scope for v0.10.0 — see WR-09 Pitfall 3 / RESEARCH.md
            // lines 668-714. Hitting it would require user code to LOAD the
            // extension from two threads simultaneously, which is atypical for
            // DuckDB's extension-load API.
            //
            // Helper-TF registrations below use `OnCreateConflict::ALTER_ON_CONFLICT`
            // and are naturally idempotent — no analog guard needed there.
            auto &cbmgr = config.GetCallbackManager();
            bool already_registered = false;
            for (auto &existing : cbmgr.ParserExtensions()) {
                if (existing.parser_override == sv_parser_override) {
                    already_registered = true;
                    break;
                }
            }

            if (!already_registered) {
                // Phase 65.1 CR-01 / WR-09: the `ParserExtension ext; ...;
                // Register(...)` block — including the `SemanticViewsParserInfo`
                // allocation — must stay INSIDE the dedup guard.
                // `ParserExtension::Register` unconditionally appends to
                // `DBConfig::parser_extensions` (no dedup API), so on the
                // `already_registered` skip path we must not Register (or
                // allocate) again — otherwise every redundant LOAD grows the
                // list by one entry, an unbounded soft leak over a long-lived
                // host process. AR-7: there is no longer a separate
                // `sv_make_override_context()` allocation to guard here.
                ParserExtension ext;
                ext.parser_override = sv_parser_override;
                // Phase 62 Plan 03: parse_function is the error-reporting layer.
                // parser_override owns the success path (rewrite + re-parse on the
                // caller's connection — transactional). When parser_override
                // defers (rc=2), the default parser fails on the unrecognised
                // prefix and DuckDB calls sv_parse_stub, which re-runs validation
                // and returns DISPLAY_EXTENSION_ERROR with `error_location` so
                // ParserException::SyntaxError renders `LINE 1: … ^` (caret).
                // sv_plan_unreachable is the required sibling — sv_parse_stub
                // never returns PARSE_SUCCESSFUL so it should never fire.
                ext.parse_function  = sv_parse_stub;
                ext.plan_function   = sv_plan_unreachable;
                // duckdb::shared_ptr<T> doesn't have an upcast constructor, so we
                // build the std::shared_ptr<ParserExtensionInfo> first (allocates
                // the SemanticViewsParserInfo and immediately upcasts) then hand
                // it to duckdb::shared_ptr's std-interop constructor.
                std::shared_ptr<ParserExtensionInfo> info_std(
                    new SemanticViewsParserInfo());
                ext.parser_info = duckdb::shared_ptr<ParserExtensionInfo>(info_std);
                ParserExtension::Register(config, ext);

                // FALLBACK_OVERRIDE: hook runs before default parser; misses
                // fall through cleanly. Caret regression (TECH-DEBT 22) is
                // resolved by parse_function above — sv_parse_stub renders
                // `LINE 1: ... ^` for every CREATE/DROP/ALTER validation error.
                config.SetOption("allow_parser_override_extension", Value("FALLBACK"));
            }

            // Phase 65 Plan 04 (Task 2 Step B): register the
            // `__sv_compute_create_from_yaml` helper TF via the C++ Catalog
            // API so its bind callback receives a native `ClientContext &`
            // and can open per-call `Connection(*context.db)` to read the
            // YAML file. The outer parser_override INSERT in
            // src/parse.rs::rewrite_yaml_file_create wraps the helper TF's
            // `new_def` row with json_merge_patch + json_object to add
            // now()/current_database()/current_schema() on the caller's
            // connection, preserving D-21 transactional contract.
            {
                // Phase 65.1 Plan 07 (IN-04 D-24): helper TF slimmed to
                // 3 args — (file_path, view_name, comment). The previous
                // `kind` INTEGER parameter was redundant with the outer
                // parser_override INSERT shape (OR IGNORE / OR REPLACE /
                // plain) and the Rust helper ignored it.
                LogicalType arg_types[] = {
                    LogicalType::VARCHAR,  // file_path
                    LogicalType::VARCHAR,  // view_name
                    LogicalType::VARCHAR,  // comment (empty = none)
                };
                // Phase 65.1 Plan 02a: sv_register_table_function requires a
                // `(error_buf, error_buf_len)` trailing pair.
                //
                // Phase 65.1 Plan 10 (WR-06 D-13): sv_register_parser_hooks
                // now carries its own error_buf; forward the helper TF's
                // diagnostic into the parent buffer so the Rust caller
                // surfaces it through `decode_register_err_buf`. The local
                // buffer is needed because the helper writes via snprintf
                // and may truncate independently of our outer buffer's size.
                char helper_err[1024];
                std::memset(helper_err, 0, sizeof(helper_err));
                if (!sv_register_table_function(
                        db_handle,
                        "__sv_compute_create_from_yaml",
                        arg_types, 3,
                        sv_create_from_yaml_bind,
                        sv_create_from_yaml_function,
                        sv_create_from_yaml_init_local,
                        helper_err, sizeof(helper_err))) {
                    if (error_buf != nullptr && error_buf_len > 0) {
                        snprintf(error_buf, error_buf_len,
                            "sv_register_parser_hooks: failed to register "
                            "__sv_compute_create_from_yaml helper TF: %s",
                            helper_err);
                    }
                    return false;
                }
            }

            return true;
        } catch (const std::exception &e) {
            if (error_buf != nullptr && error_buf_len > 0) {
                snprintf(error_buf, error_buf_len,
                    "sv_register_parser_hooks failed: %s", e.what());
            }
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// sv_count_parser_extensions -- Phase 65.1 Plan 12 Task 3 (D-21 + B-07
// plan-checker fix) structural verification helper.
// ---------------------------------------------------------------------------
// Counts entries in `DBConfig::GetCallbackManager().ParserExtensions()`
// whose `parser_override` function pointer equals the file-static
// `sv_parser_override` symbol. Used by
// `tests/parser_hook_idempotent.rs` as a stable, named symbol the
// structural test can reference (the cargo-test FFI path cannot
// exercise the extension binary directly under the
// `duckdb/loadable-extension` stubs; see that test's docstring).
//
// Option (a) per the B-07 plan-checker fix in 65.1-12-PLAN.md: PUBLIC
// helper (no cfg(test) gating). Read-only iteration is harmless in
// production binaries and gating would require conditional-compilation
// plumbing in the C++ TU for marginal benefit.
//
// Returns >= 0 on success (the count); -1 on failure with the message
// in `error_buf`. Same error_buf convention as the WR-02 ABI.
extern "C" {
    int32_t sv_count_parser_extensions(
        duckdb_database db_handle,
        char *error_buf, size_t error_buf_len) {
        try {
            if (db_handle == nullptr) {
                if (error_buf != nullptr && error_buf_len > 0) {
                    snprintf(error_buf, error_buf_len,
                        "sv_count_parser_extensions: null db_handle");
                }
                return -1;
            }
            auto *wrapper = reinterpret_cast<duckdb::DatabaseWrapper *>(
                db_handle->internal_ptr);
            if (wrapper == nullptr) {
                if (error_buf != nullptr && error_buf_len > 0) {
                    snprintf(error_buf, error_buf_len,
                        "sv_count_parser_extensions: null DatabaseWrapper");
                }
                return -1;
            }
            auto &db = *wrapper->database->instance;
            auto &config = DBConfig::GetConfig(db);
            auto &cbmgr = config.GetCallbackManager();
            // Phase 65.1 IN-02: saturate the counter at INT32_MAX rather
            // than wrapping. The documented contract is "returns >= 0 on
            // success; -1 on failure" — silent wrap to a negative value
            // would be misinterpreted as a registration failure by the
            // structural test. Reaching INT32_MAX is only plausible if
            // WR-09's dedup guard fails badly and the list grows
            // unbounded across re-LOAD cycles, but the saturation is
            // cheap defence-in-depth.
            int32_t count = 0;
            for (auto &existing : cbmgr.ParserExtensions()) {
                if (existing.parser_override == sv_parser_override) {
                    if (count < INT32_MAX) {
                        ++count;
                    }
                }
            }
            return count;
        } catch (const std::exception &e) {
            if (error_buf != nullptr && error_buf_len > 0) {
                snprintf(error_buf, error_buf_len,
                    "sv_count_parser_extensions failed: %s", e.what());
            }
            return -1;
        } catch (...) {
            if (error_buf != nullptr && error_buf_len > 0) {
                snprintf(error_buf, error_buf_len,
                    "sv_count_parser_extensions failed: unknown C++ exception");
            }
            return -1;
        }
    }
}
