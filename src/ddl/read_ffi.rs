//! Shared FFI helpers for the Phase 65 Plan 05 read-path migrations.
//!
//! Plan 05 Task 1 (Wave 0 bridge spike) established the wire-format
//! convention for handing serialized result rows from the Rust dispatcher to
//! the C++ bind callback (see `src/ddl/list.rs::sv_list_semantic_views_bind_rust`).
//! As the remaining 16 read-side migrations adopt the same pattern, this
//! module centralises the reusable pieces so each per-function dispatcher
//! shrinks to "collect rows → serialize → return".
//!
//! # Wire format (length-prefixed binary, little-endian)
//!
//! ```text
//! u32 row_count
//! for each row:
//!   for each column:
//!     u32 byte_len
//!     byte_len bytes (UTF-8 payload — VARCHAR cells)
//! ```
//!
//! Column layout (count + order + types) is implicit — agreed out-of-band
//! between the Rust dispatcher and the matching C++ bind. The C++ side
//! parses with `sv_read_u32_le` + `sv_read_string` helpers (already in
//! `cpp/src/shim.cpp` from the Wave 0 spike) and emits rows into the
//! DataChunk.
//!
//! # Variant: VARCHAR with a trailing BOOLEAN column
//!
//! `show_semantic_dimensions_for_metric` returns 3 VARCHAR + 1 BOOLEAN.
//! The wire-format encodes the BOOLEAN as a single trailing `u8` per row
//! (1 = TRUE, 0 = FALSE) after all the VARCHAR cells. C++ side parses with
//! `sv_read_u8` after the string reads.
//!
//! # Borrow contract (critical)
//!
//! Every dispatcher in this module receives a `duckdb_connection` BORROWED
//! from a stack `Connection probe(*context.db)` constructed by the C++ bind
//! callback. The Rust side MUST NOT call `duckdb_disconnect` on the handle
//! — that would `delete` a stack object (UB). The C++ bind scope's
//! `~Connection()` handles teardown.

#![cfg(feature = "extension")]

use libduckdb_sys as ffi;
use std::ffi::CString;

/// Borrowed `duckdb_connection` handle. The bridge contract: this handle is
/// owned by a stack `Connection probe(*context.db)` constructed in a C++
/// bind/exec callback. The Rust side MUST NOT call `duckdb_disconnect` —
/// doing so would `delete` a stack object (UB).
///
/// This newtype enforces the contract at compile time (Phase 65.1 D-10 /
/// WR-05): `ffi::duckdb_disconnect` accepts `*mut ffi::duckdb_connection`,
/// not `*mut BorrowedConnection`, so the call simply does not type-check.
/// Even though `BorrowedConnection` is `#[repr(transparent)]` wrapping
/// `duckdb_connection`, Rust newtype distinctness blocks the coercion at
/// the type level. Defence in depth alongside the AST-walk guard at
/// `tests/no_long_lived_conn.rs`.
///
/// Construct via the unsafe [`BorrowedConnection::new`] constructor at the
/// FFI boundary; access the raw handle via [`BorrowedConnection::as_raw`]
/// only for further FFI calls (`duckdb_query`, `duckdb_prepare`, ...).
///
/// # Negative compile coverage
///
/// The following snippet must fail to compile because `duckdb_disconnect`
/// takes `*mut duckdb_connection`, and `&mut BorrowedConnection` cannot be
/// coerced to that type — exactly the regression this newtype guards
/// against. The doctest uses a mutable binding so the failure is the
/// intended type mismatch, NOT an immutable-binding error.
///
/// ```compile_fail
/// use semantic_views::ddl::read_ffi::BorrowedConnection;
/// let mut b: BorrowedConnection = unsafe { std::mem::zeroed() };
/// // duckdb_disconnect takes *mut duckdb_connection. &mut BorrowedConnection
/// // is NOT *mut duckdb_connection, even though BorrowedConnection is
/// // #[repr(transparent)] wrapping duckdb_connection.
/// unsafe { libduckdb_sys::duckdb_disconnect(&mut b) };
/// ```
#[repr(transparent)]
pub struct BorrowedConnection(ffi::duckdb_connection);

impl BorrowedConnection {
    /// Wrap a raw FFI handle as a borrowed connection.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `conn` outlives this `BorrowedConnection`
    /// and that no other code path will call `duckdb_disconnect(conn)` while
    /// this borrow is live. Typical usage: construct immediately on entry to
    /// an `extern "C"` dispatcher from the raw `conn` parameter passed by the
    /// C++ bind/exec callback.
    #[must_use]
    pub unsafe fn new(conn: ffi::duckdb_connection) -> Self {
        Self(conn)
    }

    /// Access the underlying raw handle for further FFI calls
    /// (`duckdb_query`, `duckdb_prepare`, etc.). The caller MUST NOT pass
    /// the returned handle to `duckdb_disconnect`.
    #[must_use]
    pub fn as_raw(&self) -> ffi::duckdb_connection {
        self.0
    }

    /// Whether the wrapped handle is null. Cheaper than constructing a
    /// `CatalogReader` just to discover the bind callback handed us a
    /// null connection.
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }
}
// Deliberately NO Drop, NO Clone, NO Copy, NO trait impls that could lead
// to disconnect — the type-level guard depends on the call surface staying
// minimal. Do not add `query()` / `prepare()` convenience methods here;
// they would encourage passing BorrowedConnection around without
// unwrapping intent at each FFI boundary.

/// Probe whether `semantic_layer._definitions` exists on the given borrowed
/// connection. Returns `false` if the schema/table is missing OR if the
/// probe query itself fails (defensive — never raises). Mirrors the Phase
/// 63 read-only short-circuit logic at `src/lib.rs:393-403` and the inline
/// probe in `src/ddl/list.rs`.
///
/// # Safety
///
/// `borrowed` must wrap a valid `duckdb_connection`. The handle is borrowed
/// and must outlive this call — the typical caller is a bind dispatcher
/// running inside a C++ bind callback that owns a stack `Connection
/// probe(*context.db)`.
pub unsafe fn probe_catalog_table_present(borrowed: &BorrowedConnection) -> bool {
    let conn = borrowed.as_raw();
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
/// truncating to at most `buf_len - 1` payload bytes on a UTF-8 char
/// boundary. Thin alias for the shared [`crate::ffi_util::write_error_to_buffer`]
/// (ST-4 consolidation) kept for the ~100 dispatcher call sites; the local
/// copy it replaced truncated mid-codepoint, producing invalid UTF-8 in
/// `BinderException` text (FF-5).
///
/// # Safety
///
/// `buf` must be either null OR point to writable storage of at least
/// `buf_len` bytes.
pub unsafe fn write_err(buf: *mut u8, buf_len: usize, msg: &str) {
    crate::ffi_util::write_error_to_buffer(buf, buf_len, msg);
}

/// Serialize a vector of VARCHAR rows into the wire format described above.
///
/// `rows` is a `Vec<Vec<String>>` where every inner Vec has the same length
/// (number of columns). The function does NOT validate that — callers are
/// expected to construct rectangular row sets.
#[must_use]
pub fn serialize_varchar_rows(rows: &[Vec<String>]) -> Vec<u8> {
    let cap = 4 + rows
        .iter()
        .map(|r| r.iter().map(|s| 4 + s.len()).sum::<usize>())
        .sum::<usize>();
    let mut buf = Vec::with_capacity(cap);
    let row_count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&row_count.to_le_bytes());
    for row in rows {
        for col in row {
            let len = u32::try_from(col.len()).unwrap_or(u32::MAX);
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(col.as_bytes());
        }
    }
    buf
}

/// Serialize a vector of (VARCHAR-cells, BOOL) rows. Each row's strings are
/// emitted first (same shape as `serialize_varchar_rows`) followed by a
/// single trailing `u8` (1 = TRUE, 0 = FALSE).
#[must_use]
pub fn serialize_varchar_bool_rows(rows: &[(Vec<String>, bool)]) -> Vec<u8> {
    let cap = 4 + rows
        .iter()
        .map(|(strs, _)| strs.iter().map(|s| 4 + s.len()).sum::<usize>() + 1)
        .sum::<usize>();
    let mut buf = Vec::with_capacity(cap);
    let row_count = u32::try_from(rows.len()).unwrap_or(u32::MAX);
    buf.extend_from_slice(&row_count.to_le_bytes());
    for (strs, b) in rows {
        for col in strs {
            let len = u32::try_from(col.len()).unwrap_or(u32::MAX);
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(col.as_bytes());
        }
        buf.push(u8::from(*b));
    }
    buf
}

/// Hand a heap-owned `Vec<u8>` to the C++ side via the (ptr, len)
/// out-parameters. The C++ side MUST release the buffer with
/// `sv_free_buffer(ptr, len)` using the exact `(ptr, len)` pair this function
/// returns.
///
/// Thin alias for the shared [`crate::ffi_util::publish_owned_bytes`] (ST-4
/// consolidation), which uses the both-or-drop contract: if either
/// out-pointer is null the buffer is dropped and neither slot is written.
/// The local copy it replaced leaked the buffer and could desync
/// `(ptr, len)` by writing only one slot.
///
/// # Safety
///
/// Either both `out_ptr` and `out_len` point to writable slots, or the call
/// is treated as "drop and skip writing".
pub unsafe fn publish_owned_buffer(buf: Vec<u8>, out_ptr: *mut *mut u8, out_len: *mut usize) {
    crate::ffi_util::publish_owned_bytes(buf, out_ptr, out_len);
}

/// Shared scaffold for the read-side bind/exec dispatchers (ST-2).
///
/// Owns the boilerplate every dispatcher repeated verbatim: the
/// `catch_unwind` guard, the borrowed-connection null check, publishing the
/// success buffer, writing the error string, and the panic arm. The caller's
/// `body` does only the interesting work — parse args, read the catalog,
/// assemble + serialize rows — and returns the serialized wire buffer on
/// success or an error message.
///
/// Return-code contract (unchanged from the hand-written dispatchers):
/// `0` = success (buffer published), `1` = handled error (message in
/// `error_buf`), `2` = panic (message in `error_buf`).
///
/// # Safety
///
/// `conn` is a borrowed handle (see module-level borrow contract); it must
/// outlive the call and must not be disconnected. `out_ptr`/`out_len` are the
/// C++ out-parameters for the published buffer; `error_buf`/`error_buf_len`
/// the C++ diagnostic slot. `panic_label` names the dispatcher for the panic
/// message.
pub unsafe fn run_dispatcher<F>(
    conn: ffi::duckdb_connection,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
    error_buf: *mut u8,
    error_buf_len: usize,
    panic_label: &str,
    body: F,
) -> u8
where
    F: FnOnce(&BorrowedConnection) -> Result<Vec<u8>, String>,
{
    // AssertUnwindSafe mirrors the per-dispatcher wrapper this replaces: the
    // captured raw pointers are not `UnwindSafe`, but the catch_unwind here is
    // purely to convert a panic into rc=2 — no state is observed after unwind.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let borrowed = BorrowedConnection::new(conn);
        if borrowed.is_null() {
            write_err(error_buf, error_buf_len, "duckdb_connection is null");
            return 1_u8;
        }
        match body(&borrowed) {
            Ok(buf) => {
                publish_owned_buffer(buf, out_ptr, out_len);
                0_u8
            }
            Err(msg) => {
                write_err(error_buf, error_buf_len, &msg);
                1_u8
            }
        }
    }));
    match result {
        Ok(rc) => rc,
        Err(_) => {
            write_err(
                error_buf,
                error_buf_len,
                &format!("internal error: panic inside {panic_label}"),
            );
            2
        }
    }
}

/// Decode a `(ptr, len)` string argument passed from the C++ side, checking
/// for a null pointer and valid UTF-8 (ST-2). `what` names the argument for
/// the error message (e.g. `"view name"` → `"view name pointer is null"` /
/// `"view name is not valid UTF-8"`), matching the wording the hand-written
/// dispatchers used.
///
/// # Safety
///
/// If non-null, `ptr` must point to `len` readable bytes for the duration of
/// the call.
pub unsafe fn read_str_arg(ptr: *const u8, len: usize, what: &str) -> Result<String, String> {
    if ptr.is_null() {
        return Err(format!("{what} pointer is null"));
    }
    match std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) {
        Ok(s) => Ok(s.to_string()),
        Err(_) => Err(format!("{what} is not valid UTF-8")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_empty_row_set() {
        let buf = serialize_varchar_rows(&[]);
        assert_eq!(buf, vec![0, 0, 0, 0]);
    }

    #[test]
    fn serialize_single_row() {
        let rows = vec![vec!["a".to_string(), "bc".to_string()]];
        let buf = serialize_varchar_rows(&rows);
        let expected: Vec<u8> = vec![
            1, 0, 0, 0, // row_count = 1
            1, 0, 0, 0,    // len("a") = 1
            b'a', // "a"
            2, 0, 0, 0, // len("bc") = 2
            b'b', b'c', // "bc"
        ];
        assert_eq!(buf, expected);
    }

    #[test]
    fn serialize_bool_suffix() {
        let rows = vec![
            (vec!["x".to_string()], true),
            (vec!["y".to_string()], false),
        ];
        let buf = serialize_varchar_bool_rows(&rows);
        let expected: Vec<u8> = vec![
            2, 0, 0, 0, // row_count = 2
            1, 0, 0, 0, b'x', 1, // ("x", true)
            1, 0, 0, 0, b'y', 0, // ("y", false)
        ];
        assert_eq!(buf, expected);
    }
}
