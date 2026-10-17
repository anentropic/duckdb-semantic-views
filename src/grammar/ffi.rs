//! C-ABI entry points the C++ grammar bridge calls (`cpp/src/shim.cpp`).

use super::transform_panic_guard;
use super::tree::Tree;
use crate::ffi_util::{publish_owned_string, write_error_to_buffer};

/// FFI export: the grammar changes to register ([`super::changes`]), as a
/// heap-owned buffer the caller releases with `sv_free_buffer`.
///
/// Returns 0 on success, 1 if building the list panicked.
///
/// # Safety
///
/// `out_ptr` / `out_len` must each point to a writable slot, or both be null.
#[no_mangle]
pub unsafe extern "C" fn sv_grammar_changes_rust(out_ptr: *mut *mut u8, out_len: *mut usize) -> u8 {
    let result = std::panic::catch_unwind(super::changes);
    match result {
        Ok(text) => {
            publish_owned_string(text, out_ptr, out_len);
            0
        }
        Err(_) => 1,
    }
}

/// FFI export: lower one matched statement rule to SQL ([`super::transform`],
/// then `crate::parse::lower_action`).
///
/// `tree_ptr` / `tree_len` hold the serialised parse subtree ([`Tree::decode`]).
///
/// Returns:
///   0 = success: the SQL is written to `*sql_out_ptr` / `*sql_out_len`
///       (release with `sv_free_buffer`).
///   1 = the statement is ours but invalid: the message is in `error_out`,
///       and `*position_out` is its byte offset in the query, or `u32::MAX`.
///   2 = the bridge failed (a tree that does not decode): the message is in
///       `error_out`. A panic is rc=1 with an internal-error message, so the
///       user sees a parser error rather than an `InternalException`.
///
/// # Safety
///
/// - `tree_ptr` must point to `tree_len` readable bytes.
/// - `sql_out_ptr` / `sql_out_len` must each point to a writable slot, or both be null.
/// - `error_out` must point to `error_out_len` writable bytes.
/// - `position_out` must point to a writable `u32`, or be null.
#[no_mangle]
pub unsafe extern "C" fn sv_grammar_transform_rust(
    tree_ptr: *const u8,
    tree_len: usize,
    sql_out_ptr: *mut *mut u8,
    sql_out_len: *mut usize,
    error_out: *mut u8,
    error_out_len: usize,
    position_out: *mut u32,
) -> u8 {
    transform_panic_guard(error_out, error_out_len, position_out, || {
        if tree_ptr.is_null() {
            write_error_to_buffer(error_out, error_out_len, "empty parse tree");
            return 2;
        }
        let bytes = std::slice::from_raw_parts(tree_ptr, tree_len);
        let tree = match Tree::decode(bytes) {
            Ok(tree) => tree,
            Err(message) => {
                write_error_to_buffer(error_out, error_out_len, &message);
                return 2;
            }
        };
        match super::transform(&tree).and_then(crate::parse::lower_action) {
            Ok(sql) => {
                publish_owned_string(sql, sql_out_ptr, sql_out_len);
                0
            }
            Err(err) => {
                write_error_to_buffer(error_out, error_out_len, &err.message);
                if !position_out.is_null() {
                    *position_out = err
                        .position
                        .and_then(|p| u32::try_from(p).ok())
                        .unwrap_or(u32::MAX);
                }
                1
            }
        }
    })
}
