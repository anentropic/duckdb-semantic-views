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

/// Probe whether `semantic_layer._definitions` exists on the given borrowed
/// connection. Returns `false` if the schema/table is missing OR if the
/// probe query itself fails (defensive — never raises). Mirrors the Phase
/// 63 read-only short-circuit logic at `src/lib.rs:393-403` and the inline
/// probe in `src/ddl/list.rs`.
///
/// # Safety
///
/// `conn` must be a valid `duckdb_connection`. The handle is borrowed and
/// must outlive this call — the typical caller is a bind dispatcher running
/// inside a C++ bind callback that owns a stack `Connection probe(*context.db)`.
pub unsafe fn probe_catalog_table_present(conn: ffi::duckdb_connection) -> bool {
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
/// `src/ddl/alter_helpers_ffi.rs::write_error_buf` and `src/ddl/list.rs`.
///
/// # Safety
///
/// `buf` must be either null OR point to writable storage of at least
/// `buf_len` bytes.
pub unsafe fn write_err(buf: *mut u8, buf_len: usize, msg: &str) {
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
/// # Safety
///
/// `out_ptr` and `out_len` must be writable pointers (or null — in which
/// case the buffer leaks; defensive only). The function transfers
/// allocation ownership across the FFI boundary.
pub unsafe fn publish_owned_buffer(buf: Vec<u8>, out_ptr: *mut *mut u8, out_len: *mut usize) {
    let boxed: Box<[u8]> = buf.into_boxed_slice();
    let len = boxed.len();
    let raw = Box::into_raw(boxed) as *mut u8;
    if !out_ptr.is_null() {
        *out_ptr = raw;
    }
    if !out_len.is_null() {
        *out_len = len;
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
