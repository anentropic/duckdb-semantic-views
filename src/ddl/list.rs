use std::sync::atomic::{AtomicBool, Ordering};

use duckdb::{
    core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId},
    vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab},
};

use crate::catalog::CatalogReader;
use crate::model::SemanticViewDefinition;

// ---------------------------------------------------------------------------
// list_semantic_views — Phase 65 Plan 05 Task 1 (Wave 0 bridge spike)
// ---------------------------------------------------------------------------
//
// The legacy `ListSemanticViewsVTab` below (`impl VTab`) was registered via
// duckdb-rs's `register_table_function_with_extra_info`, which marshals
// `ClientContext &` away (Plan 01 Spike A6). That registration path is
// RETIRED for `list_semantic_views`; the function is now registered via the
// C++ Catalog API path (`cpp/src/shim.cpp::sv_register_list_semantic_views`
// → `sv_register_table_function`). The bind callback opens a per-call
// `Connection(*context.db)` and bridges to Rust via
// `sv_list_semantic_views_bind_rust` (defined below).
//
// The bridge mechanism is a `reinterpret_cast` of the stack `Connection *`
// to `duckdb_connection` — confirmed by reading `duckdb.cpp:266432-266447`
// where `duckdb_connect` is literally
// `reinterpret_cast<duckdb_connection>(new Connection(...))`. The Rust
// dispatcher receives a BORROWED handle: it MUST NOT call
// `duckdb_disconnect` (would `delete` the stack-allocated Connection — UB).
// The C++ bind scope's `~Connection()` handles teardown.
//
// The struct + impl block for `ListSemanticViewsVTab` is left in place
// (marked `#[allow(dead_code)]`) until the remaining 16 read-side
// migrations (Tasks 2-6) validate the spike pattern. Once all read-side
// migrations are complete the struct + impl + BindData + InitData can be
// deleted as a single cleanup commit. The `ListTerseSemanticViewsVTab`
// further down stays live until Task 2 (Wave 1) migrates it.

/// FFI entry point: read the catalog and serialize all semantic views as a
/// length-prefixed binary buffer for the C++ bind callback to parse.
///
/// Called from `cpp/src/shim.cpp::sv_list_semantic_views_bind`. The C++
/// side passes a per-call `duckdb_connection` borrowed from a stack
/// `Connection probe(*context.db)`. This function wraps the handle in a
/// `CatalogReader`, performs the catalog read, and serializes the rows
/// into a flat binary buffer with the wire format:
///
///   u32 row_count (little-endian)
///   for each row:
///     for each of 6 columns:
///       u32 byte_len (little-endian)
///       byte_len bytes (UTF-8, NOT NUL-terminated)
///
/// The 6 columns match the v0.9.0 Rust VTab shape exactly:
/// (created_on, name, kind, database_name, schema_name, comment).
///
/// # Bridge lifecycle (critical)
///
/// The `conn` parameter is a BORROWED handle — the underlying C++
/// `Connection` is owned by a stack local in the C++ bind callback.
/// This function MUST NOT:
///   * call `duckdb_disconnect(conn)` (would `delete` a stack object — UB),
///   * stash the handle in long-lived storage (would dangle after bind),
///   * call functions that take ownership of the handle (none in the
///     CatalogReader path — `CatalogReader::new` only stores the raw
///     pointer, and the prepared-statement / query helpers in
///     `src/catalog.rs` operate on the handle without consuming it).
///
/// # Return codes
///
/// * `0` — success; `(out_ptr, out_len)` populated. Caller MUST release
///         via `sv_free_buffer(ptr, len)`.
/// * `1` — catalog read error (e.g. the `semantic_layer._definitions`
///         table is missing); `error_buf` populated.
/// * `2` — internal error (panic across FFI, serialization failure);
///         `error_buf` populated.
///
/// # Catalog-table-present probing
///
/// The original `init_extension` probes whether `semantic_layer._definitions`
/// exists once at LOAD time and passes the flag to `CatalogReader::new`.
/// Under the per-call connection model that probe can't be done at LOAD
/// time anymore (the per-call conn doesn't exist yet). We probe inline
/// here on every bind: cheap (single `information_schema.tables` lookup)
/// and correct against schema drift mid-session. The probe runs on the
/// same per-call connection so it shares the caller's catalog/search-path
/// view (matches the Phase 63 read-only short-circuit behavior).
#[cfg(feature = "extension")]
#[no_mangle]
pub unsafe extern "C" fn sv_list_semantic_views_bind_rust(
    conn: libduckdb_sys::duckdb_connection,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
) -> u8 {
    use std::panic::AssertUnwindSafe;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if conn.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }

        // Probe whether semantic_layer._definitions exists on the caller's
        // connection. Cheap (single query); matches Phase 63's RO-load
        // short-circuit semantics so an attached read-only DB without a
        // bootstrapped catalog returns 0 rows instead of an error.
        let table_present = probe_catalog_table_present(conn);

        // Wrap the borrowed handle. CatalogReader::new only stores the
        // raw pointer — no transfer of ownership.
        let reader = CatalogReader::new(conn, table_present);
        let entries = match reader.list_all() {
            Ok(e) => e,
            Err(e) => {
                write_err(error_buf, error_buf_len, &e);
                return 1_u8;
            }
        };

        // Reconstruct the 6-column rows exactly like the Rust VTab did
        // (ListBindData::rows) — sort by name so output ordering is
        // byte-identical to the v0.9.0 behavior.
        let mut rows: Vec<[String; 6]> = Vec::with_capacity(entries.len());
        for (name, json) in &entries {
            let def = SemanticViewDefinition::from_json(name, json).ok();
            let (created_on, database_name, schema_name, comment) = match &def {
                Some(d) => (
                    d.created_on.clone().unwrap_or_default(),
                    d.database_name.clone().unwrap_or_default(),
                    d.schema_name.clone().unwrap_or_default(),
                    d.comment.clone().unwrap_or_default(),
                ),
                None => (String::new(), String::new(), String::new(), String::new()),
            };
            rows.push([
                created_on,
                name.clone(),
                "SEMANTIC_VIEW".to_string(),
                database_name,
                schema_name,
                comment,
            ]);
        }
        rows.sort_by(|a, b| a[1].cmp(&b[1]));

        // Serialize to the flat binary buffer. Wire format:
        //   u32 row_count (LE)
        //   for each row:
        //     for each of 6 cols:
        //       u32 byte_len (LE) | bytes
        let row_count = rows.len() as u32;
        let mut buf: Vec<u8> = Vec::with_capacity(
            4 + rows
                .iter()
                .map(|r| r.iter().map(|s| 4 + s.len()).sum::<usize>())
                .sum::<usize>(),
        );
        buf.extend_from_slice(&row_count.to_le_bytes());
        for row in &rows {
            for col in row {
                let len = col.len() as u32;
                buf.extend_from_slice(&len.to_le_bytes());
                buf.extend_from_slice(col.as_bytes());
            }
        }

        // Hand the heap-owned buffer to the C++ side. Caller releases via
        // sv_free_buffer with the exact (ptr, len) pair. Matches the
        // convention established in src/parse.rs and src/ddl/alter_helpers_ffi.rs.
        let boxed: Box<[u8]> = buf.into_boxed_slice();
        let len = boxed.len();
        let raw = Box::into_raw(boxed) as *mut u8;
        if !out_ptr.is_null() {
            *out_ptr = raw;
        }
        if !out_len.is_null() {
            *out_len = len;
        }
        0_u8
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            write_err(
                error_buf,
                error_buf_len,
                "internal error: panic inside sv_list_semantic_views_bind_rust",
            );
            2
        }
    }
}

/// Probe whether `semantic_layer._definitions` exists on the given borrowed
/// connection. Returns `false` if the schema/table is missing OR if the
/// probe query itself fails (defensive — never raises). Mirrors the Phase
/// 63 read-only short-circuit logic at `src/lib.rs:393-403`.
#[cfg(feature = "extension")]
unsafe fn probe_catalog_table_present(conn: libduckdb_sys::duckdb_connection) -> bool {
    use libduckdb_sys as ffi;
    use std::ffi::CString;
    let sql = match CString::new(
        "SELECT 1 FROM information_schema.tables \
         WHERE table_schema = 'semantic_layer' AND table_name = '_definitions' LIMIT 1",
    ) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut result: ffi::duckdb_result = std::mem::zeroed();
    let rc = ffi::duckdb_query(conn, sql.as_ptr(), &mut result);
    let present = if rc == ffi::DuckDBSuccess {
        ffi::duckdb_row_count(&mut result) > 0
    } else {
        false
    };
    ffi::duckdb_destroy_result(&mut result);
    present
}

/// Write a NUL-terminated error message into the C-side `error_buf`,
/// truncating to `buf_len - 1` payload bytes. Matches the convention in
/// `src/ddl/alter_helpers_ffi.rs::write_error_buf`.
#[cfg(feature = "extension")]
unsafe fn write_err(buf: *mut u8, buf_len: usize, msg: &str) {
    if buf.is_null() || buf_len == 0 {
        return;
    }
    let max = buf_len.saturating_sub(1);
    let bytes = msg.as_bytes();
    let n = bytes.len().min(max);
    if n > 0 {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, n);
    }
    *buf.add(n) = 0;
}

// ---------------------------------------------------------------------------
// Legacy Rust VTab — retired by Plan 05 Task 1; kept for one wave for review
// ---------------------------------------------------------------------------
// The struct + impl below were the registration target for
// `register_table_function_with_extra_info`. Task 1 retires that
// registration in favor of the C++ Catalog API path (above). The struct
// stays in place under `#[allow(dead_code)]` until all 17 read-side
// migrations land — at that point a single cleanup commit can delete
// `ListSemanticViewsVTab`, `ListBindData`, `ListInitData`, and the
// `unsafe impl Send/Sync` lines.

/// A single row in the SHOW SEMANTIC VIEWS output.
#[allow(dead_code)]
struct ListRow {
    created_on: String,
    name: String,
    kind: String,
    database_name: String,
    schema_name: String,
    comment: String,
}

/// Bind-time snapshot of all registered semantic views.
///
/// Populated once at bind time by reading the in-memory catalog.
/// Stored as a `Vec<ListRow>` — one entry per view with 6 Snowflake-aligned columns:
/// created_on, name, kind, database_name, schema_name, comment.
#[allow(dead_code)]
pub struct ListBindData {
    rows: Vec<ListRow>,
}

// SAFETY: `Vec<ListRow>` contains only `String` fields, which are `Send + Sync`.
unsafe impl Send for ListBindData {}
unsafe impl Sync for ListBindData {}

/// Init data for `list_semantic_views`: tracks whether rows have been emitted.
#[allow(dead_code)]
pub struct ListInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ListInitData {}
unsafe impl Sync for ListInitData {}

/// Table function that returns 6 columns: `(created_on VARCHAR, name VARCHAR,
/// kind VARCHAR, database_name VARCHAR, schema_name VARCHAR, comment VARCHAR)` — one row per
/// registered semantic view.
///
/// Takes no parameters.  State is injected via
/// `register_table_function_with_extra_info`.
///
/// Phase 65 Plan 05 Task 1: registration retired in favor of the C++
/// Catalog API path. Struct kept for one wave; deleted after Task 6.
#[allow(dead_code)]
pub struct ListSemanticViewsVTab;

impl VTab for ListSemanticViewsVTab {
    type BindData = ListBindData;
    type InitData = ListInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind.add_result_column(
                "created_on",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column("kind", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column(
                "database_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "schema_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("comment", LogicalTypeHandle::from(LogicalTypeId::Varchar));

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let entries = reader
                .list_all()
                .map_err(Box::<dyn std::error::Error>::from)?;

            let mut rows = Vec::with_capacity(entries.len());
            for (name, json) in &entries {
                let def = SemanticViewDefinition::from_json(name, json).ok();
                let (created_on, database_name, schema_name, comment) = match &def {
                    Some(d) => (
                        d.created_on.clone().unwrap_or_default(),
                        d.database_name.clone().unwrap_or_default(),
                        d.schema_name.clone().unwrap_or_default(),
                        d.comment.clone().unwrap_or_default(),
                    ),
                    None => (String::new(), String::new(), String::new(), String::new()),
                };
                rows.push(ListRow {
                    created_on,
                    name: name.clone(),
                    kind: "SEMANTIC_VIEW".to_string(),
                    database_name,
                    schema_name,
                    comment,
                });
            }
            rows.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(ListBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ListInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();
        if init_data.done.swap(true, Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }

        let bind_data = func.get_bind_data();
        let n = bind_data.rows.len();

        let created_on_vec = output.flat_vector(0);
        let name_vec = output.flat_vector(1);
        let kind_vec = output.flat_vector(2);
        let db_name_vec = output.flat_vector(3);
        let sch_name_vec = output.flat_vector(4);
        let comment_vec = output.flat_vector(5);
        for (i, row) in bind_data.rows.iter().enumerate() {
            created_on_vec.insert(i, row.created_on.as_str());
            name_vec.insert(i, row.name.as_str());
            kind_vec.insert(i, row.kind.as_str());
            db_name_vec.insert(i, row.database_name.as_str());
            sch_name_vec.insert(i, row.schema_name.as_str());
            comment_vec.insert(i, row.comment.as_str());
        }
        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        None
    }
}

// ---------------------------------------------------------------------------
// SHOW TERSE SEMANTIC VIEWS — 5-column subset (no comment)
// ---------------------------------------------------------------------------

/// A single row in the SHOW TERSE SEMANTIC VIEWS output.
struct ListTerseRow {
    created_on: String,
    name: String,
    kind: String,
    database_name: String,
    schema_name: String,
}

/// Bind-time snapshot for terse listing.
pub struct ListTerseBindData {
    rows: Vec<ListTerseRow>,
}

// SAFETY: `Vec<ListTerseRow>` contains only `String` fields, which are `Send + Sync`.
unsafe impl Send for ListTerseBindData {}
unsafe impl Sync for ListTerseBindData {}

/// Init data for `list_terse_semantic_views`.
pub struct ListTerseInitData {
    done: AtomicBool,
}

// SAFETY: `AtomicBool` is `Send + Sync`.
unsafe impl Send for ListTerseInitData {}
unsafe impl Sync for ListTerseInitData {}

/// Table function that returns 5 columns: `(created_on VARCHAR, name VARCHAR,
/// kind VARCHAR, database_name VARCHAR, schema_name VARCHAR)` — one row per
/// registered semantic view. No comment column (terse mode).
pub struct ListTerseSemanticViewsVTab;

impl VTab for ListTerseSemanticViewsVTab {
    type BindData = ListTerseBindData;
    type InitData = ListTerseInitData;

    fn bind(bind: &BindInfo) -> Result<Self::BindData, Box<dyn std::error::Error>> {
        crate::util::catch_unwind_to_result(std::panic::AssertUnwindSafe(|| {
            bind.add_result_column(
                "created_on",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column("name", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column("kind", LogicalTypeHandle::from(LogicalTypeId::Varchar));
            bind.add_result_column(
                "database_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );
            bind.add_result_column(
                "schema_name",
                LogicalTypeHandle::from(LogicalTypeId::Varchar),
            );

            let state_ptr = bind.get_extra_info::<CatalogReader>();
            let reader = unsafe { *state_ptr };
            let entries = reader
                .list_all()
                .map_err(Box::<dyn std::error::Error>::from)?;

            let mut rows = Vec::with_capacity(entries.len());
            for (name, json) in &entries {
                let def = SemanticViewDefinition::from_json(name, json).ok();
                let (created_on, database_name, schema_name) = match &def {
                    Some(d) => (
                        d.created_on.clone().unwrap_or_default(),
                        d.database_name.clone().unwrap_or_default(),
                        d.schema_name.clone().unwrap_or_default(),
                    ),
                    None => (String::new(), String::new(), String::new()),
                };
                rows.push(ListTerseRow {
                    created_on,
                    name: name.clone(),
                    kind: "SEMANTIC_VIEW".to_string(),
                    database_name,
                    schema_name,
                });
            }
            rows.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(ListTerseBindData { rows })
        }))
    }

    fn init(_: &InitInfo) -> Result<Self::InitData, Box<dyn std::error::Error>> {
        Ok(ListTerseInitData {
            done: AtomicBool::new(false),
        })
    }

    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let init_data = func.get_init_data();
        if init_data.done.swap(true, Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }

        let bind_data = func.get_bind_data();
        let n = bind_data.rows.len();

        let created_on_vec = output.flat_vector(0);
        let name_vec = output.flat_vector(1);
        let kind_vec = output.flat_vector(2);
        let db_name_vec = output.flat_vector(3);
        let sch_name_vec = output.flat_vector(4);
        for (i, row) in bind_data.rows.iter().enumerate() {
            created_on_vec.insert(i, row.created_on.as_str());
            name_vec.insert(i, row.name.as_str());
            kind_vec.insert(i, row.kind.as_str());
            db_name_vec.insert(i, row.database_name.as_str());
            sch_name_vec.insert(i, row.schema_name.as_str());
        }
        output.set_len(n);
        Ok(())
    }

    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        None
    }
}
