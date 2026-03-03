//! Tests for `duckdb_vector_reference_vector` lifetime safety.
//!
//! Verifies whether referencing a vector from a source chunk into a newly created
//! output chunk shares ownership (safe) or is a shallow alias (unsafe after source
//! destruction). Also tests `duckdb_vector_copy_sel` as the fallback approach.
//!
//! These tests run under the default `bundled` feature (`cargo test`).

use libduckdb_sys as ffi;
use semantic_views::test_helpers::{execute_sql_raw, read_typed_value, RawDb, TestValue};

// ---------------------------------------------------------------------------
// Helper: create an identity selection vector [0, 1, 2, ..., n-1]
// ---------------------------------------------------------------------------

unsafe fn create_identity_sel(n: usize) -> ffi::duckdb_selection_vector {
    let sel = ffi::duckdb_create_selection_vector(n as ffi::idx_t);
    let data = ffi::duckdb_selection_vector_get_data_ptr(sel);
    for i in 0..n {
        *data.add(i) = i as ffi::sel_t;
    }
    sel
}

// ---------------------------------------------------------------------------
// Test: vector_reference_vector lifetime safety
// ---------------------------------------------------------------------------

/// Test that `duckdb_vector_reference_vector` creates shared ownership.
///
/// Protocol:
/// 1. Execute a query returning data
/// 2. Get a source chunk from the result
/// 3. Create a new output data chunk
/// 4. Reference source vectors into output vectors
/// 5. Destroy the source chunk
/// 6. Read values from the output chunk — if correct, ownership is shared
#[test]
fn vector_reference_survives_source_destruction() {
    let db = RawDb::open_in_memory();
    unsafe {
        db.exec("CREATE TABLE ref_test (i INTEGER, v VARCHAR, d DATE)");
        db.exec(
            "INSERT INTO ref_test VALUES \
             (42, 'hello', '2024-01-15'), \
             (NULL, 'world', '1970-01-01'), \
             (7, NULL, '1969-12-31')",
        );

        let mut result = execute_sql_raw(db.conn, "SELECT i, v, d FROM ref_test ORDER BY rowid")
            .expect("query failed");

        let col_count = ffi::duckdb_column_count(&mut result) as usize;
        assert_eq!(col_count, 3);

        // Get logical types for creating the output chunk.
        let mut logical_types: Vec<ffi::duckdb_logical_type> = Vec::with_capacity(col_count);
        for i in 0..col_count {
            logical_types.push(ffi::duckdb_column_logical_type(
                &mut result,
                i as ffi::idx_t,
            ));
        }

        // Get the source chunk.
        let chunk_count = ffi::duckdb_result_chunk_count(result);
        assert!(chunk_count >= 1, "Expected at least one chunk");

        let mut src_chunk = ffi::duckdb_result_get_chunk(result, 0);
        assert!(!src_chunk.is_null());
        let row_count = ffi::duckdb_data_chunk_get_size(src_chunk) as usize;
        assert_eq!(row_count, 3);

        // Create a new output chunk with the same schema.
        let out_chunk =
            ffi::duckdb_create_data_chunk(logical_types.as_mut_ptr(), col_count as ffi::idx_t);
        assert!(!out_chunk.is_null());

        // Reference each source vector into the output chunk.
        for col_idx in 0..col_count {
            let src_vec = ffi::duckdb_data_chunk_get_vector(src_chunk, col_idx as ffi::idx_t);
            let dst_vec = ffi::duckdb_data_chunk_get_vector(out_chunk, col_idx as ffi::idx_t);
            ffi::duckdb_vector_reference_vector(dst_vec, src_vec);
        }
        ffi::duckdb_data_chunk_set_size(out_chunk, row_count as ffi::idx_t);

        // DESTROY the source chunk — this is the critical test.
        ffi::duckdb_destroy_data_chunk(&mut src_chunk);

        // Now read from the OUTPUT chunk — if reference_vector shares ownership,
        // values should still be correct.
        // Column 0: INTEGER
        let lt0 = logical_types[0];
        let type0 = ffi::duckdb_get_type_id(lt0) as u32;
        assert_eq!(
            read_typed_value(out_chunk, 0, 0, type0, lt0),
            TestValue::I32(42)
        );
        assert_eq!(
            read_typed_value(out_chunk, 0, 1, type0, lt0),
            TestValue::Null
        );
        assert_eq!(
            read_typed_value(out_chunk, 0, 2, type0, lt0),
            TestValue::I32(7)
        );

        // Column 1: VARCHAR
        let lt1 = logical_types[1];
        let type1 = ffi::duckdb_get_type_id(lt1) as u32;
        assert_eq!(
            read_typed_value(out_chunk, 1, 0, type1, lt1),
            TestValue::Str("hello".to_string())
        );
        assert_eq!(
            read_typed_value(out_chunk, 1, 1, type1, lt1),
            TestValue::Str("world".to_string())
        );
        assert_eq!(
            read_typed_value(out_chunk, 1, 2, type1, lt1),
            TestValue::Null
        );

        // Column 2: DATE
        let lt2 = logical_types[2];
        let type2 = ffi::duckdb_get_type_id(lt2) as u32;
        assert_eq!(
            read_typed_value(out_chunk, 2, 0, type2, lt2),
            TestValue::I32(19737) // 2024-01-15
        );
        assert_eq!(
            read_typed_value(out_chunk, 2, 1, type2, lt2),
            TestValue::I32(0) // 1970-01-01
        );
        assert_eq!(
            read_typed_value(out_chunk, 2, 2, type2, lt2),
            TestValue::I32(-1) // 1969-12-31
        );

        // Cleanup
        ffi::duckdb_destroy_data_chunk(&mut { out_chunk });
        for lt in &mut logical_types {
            ffi::duckdb_destroy_logical_type(lt);
        }
        ffi::duckdb_destroy_result(&mut result);
    }
}

/// Test vector_reference_vector with >2048 rows (multiple chunks).
#[test]
fn vector_reference_multi_chunk() {
    let db = RawDb::open_in_memory();
    let n_rows = 5000_usize;
    unsafe {
        db.exec("CREATE TABLE big_ref (i INTEGER)");
        // Use generate_series to create >2048 rows.
        db.exec(&format!(
            "INSERT INTO big_ref SELECT i::INTEGER FROM generate_series(0, {}) AS t(i)",
            n_rows - 1
        ));

        let mut result =
            execute_sql_raw(db.conn, "SELECT i FROM big_ref ORDER BY i").expect("query failed");

        let chunk_count = ffi::duckdb_result_chunk_count(result) as usize;
        assert!(
            chunk_count > 1,
            "Expected multiple chunks for {n_rows} rows, got {chunk_count}"
        );

        let mut logical_types = vec![ffi::duckdb_column_logical_type(&mut result, 0)];
        let lt = logical_types[0];
        let type_id = ffi::duckdb_get_type_id(lt) as u32;

        let mut total_read = 0_usize;
        for chunk_idx in 0..chunk_count {
            let mut src_chunk = ffi::duckdb_result_get_chunk(result, chunk_idx as ffi::idx_t);
            assert!(!src_chunk.is_null());
            let row_count = ffi::duckdb_data_chunk_get_size(src_chunk) as usize;

            // Create output chunk, reference, destroy source, then read.
            let out_chunk = ffi::duckdb_create_data_chunk(logical_types.as_mut_ptr(), 1);
            let src_vec = ffi::duckdb_data_chunk_get_vector(src_chunk, 0);
            let dst_vec = ffi::duckdb_data_chunk_get_vector(out_chunk, 0);
            ffi::duckdb_vector_reference_vector(dst_vec, src_vec);
            ffi::duckdb_data_chunk_set_size(out_chunk, row_count as ffi::idx_t);

            // Destroy source chunk.
            ffi::duckdb_destroy_data_chunk(&mut src_chunk);

            // Verify values from output chunk.
            for row_idx in 0..row_count {
                let val = read_typed_value(out_chunk, 0, row_idx, type_id, lt);
                let expected = (total_read + row_idx) as i32;
                assert_eq!(
                    val,
                    TestValue::I32(expected),
                    "Mismatch at chunk {chunk_idx} row {row_idx} (global row {})",
                    total_read + row_idx
                );
            }
            total_read += row_count;

            ffi::duckdb_destroy_data_chunk(&mut { out_chunk });
        }
        assert_eq!(total_read, n_rows);

        for lt in &mut logical_types {
            ffi::duckdb_destroy_logical_type(lt);
        }
        ffi::duckdb_destroy_result(&mut result);
    }
}

/// Test vector_reference_vector with LIST and STRUCT types.
#[test]
fn vector_reference_complex_types() {
    let db = RawDb::open_in_memory();
    unsafe {
        db.exec("CREATE TABLE complex_ref (l INTEGER[], s STRUCT(a INTEGER, b VARCHAR))");
        db.exec("INSERT INTO complex_ref VALUES ([1, 2, 3], {'a': 10, 'b': 'hello'})");
        db.exec("INSERT INTO complex_ref VALUES ([4], {'a': 20, 'b': 'world'})");
        db.exec("INSERT INTO complex_ref VALUES (NULL, NULL)");

        let mut result = execute_sql_raw(db.conn, "SELECT l, s FROM complex_ref ORDER BY rowid")
            .expect("query failed");

        let col_count = ffi::duckdb_column_count(&mut result) as usize;
        assert_eq!(col_count, 2);

        let mut logical_types: Vec<ffi::duckdb_logical_type> = (0..col_count)
            .map(|i| ffi::duckdb_column_logical_type(&mut result, i as ffi::idx_t))
            .collect();

        let mut src_chunk = ffi::duckdb_result_get_chunk(result, 0);
        assert!(!src_chunk.is_null());
        let row_count = ffi::duckdb_data_chunk_get_size(src_chunk) as usize;

        let out_chunk =
            ffi::duckdb_create_data_chunk(logical_types.as_mut_ptr(), col_count as ffi::idx_t);
        for col_idx in 0..col_count {
            let src_vec = ffi::duckdb_data_chunk_get_vector(src_chunk, col_idx as ffi::idx_t);
            let dst_vec = ffi::duckdb_data_chunk_get_vector(out_chunk, col_idx as ffi::idx_t);
            ffi::duckdb_vector_reference_vector(dst_vec, src_vec);
        }
        ffi::duckdb_data_chunk_set_size(out_chunk, row_count as ffi::idx_t);

        // Destroy source.
        ffi::duckdb_destroy_data_chunk(&mut src_chunk);

        // Verify LIST column (col 0) — read raw list entries from the output chunk.
        let list_vec = ffi::duckdb_data_chunk_get_vector(out_chunk, 0);
        let list_data = ffi::duckdb_vector_get_data(list_vec);

        // Row 0: [1, 2, 3]
        let entry0 = *list_data.cast::<ffi::duckdb_list_entry>().add(0);
        assert_eq!(entry0.length, 3, "Row 0 list length");
        // Row 1: [4]
        let entry1 = *list_data.cast::<ffi::duckdb_list_entry>().add(1);
        assert_eq!(entry1.length, 1, "Row 1 list length");
        // Row 2: NULL
        let validity = ffi::duckdb_vector_get_validity(list_vec);
        if !validity.is_null() {
            let entry = *validity.add(0); // row 2 is in the first u64
            assert_eq!(entry & (1u64 << 2), 0, "Row 2 should be NULL");
        }

        // Verify child values of the list.
        let child_vec = ffi::duckdb_list_vector_get_child(list_vec);
        let child_data = ffi::duckdb_vector_get_data(child_vec);
        // Row 0 children: [1, 2, 3] at offset 0
        assert_eq!(*child_data.cast::<i32>().add(0), 1);
        assert_eq!(*child_data.cast::<i32>().add(1), 2);
        assert_eq!(*child_data.cast::<i32>().add(2), 3);
        // Row 1 children: [4] at offset 3
        assert_eq!(*child_data.cast::<i32>().add(3), 4);

        // Verify STRUCT column (col 1) — read struct children.
        let struct_vec = ffi::duckdb_data_chunk_get_vector(out_chunk, 1);
        let child_a = ffi::duckdb_struct_vector_get_child(struct_vec, 0); // INTEGER
        let child_b = ffi::duckdb_struct_vector_get_child(struct_vec, 1); // VARCHAR

        // Row 0: {a: 10, b: 'hello'}
        let a_data = ffi::duckdb_vector_get_data(child_a);
        assert_eq!(*a_data.cast::<i32>().add(0), 10);
        assert_eq!(*a_data.cast::<i32>().add(1), 20);

        // VARCHAR child b — read 'hello' and 'world'
        let b_data = ffi::duckdb_vector_get_data(child_b);
        let string_t_0 = &*b_data.cast::<ffi::duckdb_string_t>().add(0);
        let len0 = string_t_0.value.inlined.length as usize;
        assert_eq!(len0, 5); // "hello"
        let bytes0 = std::slice::from_raw_parts(
            string_t_0.value.inlined.inlined.as_ptr().cast::<u8>(),
            len0,
        );
        assert_eq!(std::str::from_utf8(bytes0).unwrap(), "hello");

        // Row 2 STRUCT: NULL
        let struct_validity = ffi::duckdb_vector_get_validity(struct_vec);
        if !struct_validity.is_null() {
            let entry = *struct_validity.add(0);
            assert_eq!(entry & (1u64 << 2), 0, "Row 2 struct should be NULL");
        }

        ffi::duckdb_destroy_data_chunk(&mut { out_chunk });
        for lt in &mut logical_types {
            ffi::duckdb_destroy_logical_type(lt);
        }
        ffi::duckdb_destroy_result(&mut result);
    }
}

// ---------------------------------------------------------------------------
// Test: vector_copy_sel fallback
// ---------------------------------------------------------------------------

/// Test `duckdb_vector_copy_sel` as fallback — always safe regardless of ownership model.
#[test]
fn vector_copy_sel_basic() {
    let db = RawDb::open_in_memory();
    unsafe {
        db.exec("CREATE TABLE copy_test (i INTEGER, v VARCHAR)");
        db.exec("INSERT INTO copy_test VALUES (1, 'alpha'), (2, 'beta'), (NULL, 'gamma')");

        let mut result = execute_sql_raw(db.conn, "SELECT i, v FROM copy_test ORDER BY rowid")
            .expect("query failed");

        let col_count = ffi::duckdb_column_count(&mut result) as usize;
        let mut logical_types: Vec<ffi::duckdb_logical_type> = (0..col_count)
            .map(|i| ffi::duckdb_column_logical_type(&mut result, i as ffi::idx_t))
            .collect();

        let mut src_chunk = ffi::duckdb_result_get_chunk(result, 0);
        let row_count = ffi::duckdb_data_chunk_get_size(src_chunk) as usize;

        // Create output chunk.
        let out_chunk =
            ffi::duckdb_create_data_chunk(logical_types.as_mut_ptr(), col_count as ffi::idx_t);

        // Create identity selection vector.
        let sel = create_identity_sel(row_count);

        // Copy each column.
        for col_idx in 0..col_count {
            let src_vec = ffi::duckdb_data_chunk_get_vector(src_chunk, col_idx as ffi::idx_t);
            let dst_vec = ffi::duckdb_data_chunk_get_vector(out_chunk, col_idx as ffi::idx_t);
            ffi::duckdb_vector_copy_sel(src_vec, dst_vec, sel, row_count as ffi::idx_t, 0, 0);
        }
        ffi::duckdb_data_chunk_set_size(out_chunk, row_count as ffi::idx_t);

        // Destroy source chunk AND selection vector — output must still be valid.
        ffi::duckdb_destroy_data_chunk(&mut src_chunk);
        ffi::duckdb_destroy_selection_vector(sel);

        // Read values from output.
        let lt0 = logical_types[0];
        let type0 = ffi::duckdb_get_type_id(lt0) as u32;
        assert_eq!(
            read_typed_value(out_chunk, 0, 0, type0, lt0),
            TestValue::I32(1)
        );
        assert_eq!(
            read_typed_value(out_chunk, 0, 1, type0, lt0),
            TestValue::I32(2)
        );
        assert_eq!(
            read_typed_value(out_chunk, 0, 2, type0, lt0),
            TestValue::Null
        );

        let lt1 = logical_types[1];
        let type1 = ffi::duckdb_get_type_id(lt1) as u32;
        assert_eq!(
            read_typed_value(out_chunk, 1, 0, type1, lt1),
            TestValue::Str("alpha".to_string())
        );
        assert_eq!(
            read_typed_value(out_chunk, 1, 1, type1, lt1),
            TestValue::Str("beta".to_string())
        );
        assert_eq!(
            read_typed_value(out_chunk, 1, 2, type1, lt1),
            TestValue::Str("gamma".to_string())
        );

        ffi::duckdb_destroy_data_chunk(&mut { out_chunk });
        for lt in &mut logical_types {
            ffi::duckdb_destroy_logical_type(lt);
        }
        ffi::duckdb_destroy_result(&mut result);
    }
}

/// Test `duckdb_vector_copy_sel` with >2048 rows (multiple chunks).
#[test]
fn vector_copy_sel_multi_chunk() {
    let db = RawDb::open_in_memory();
    let n_rows = 5000_usize;
    unsafe {
        db.exec("CREATE TABLE big_copy (i INTEGER)");
        db.exec(&format!(
            "INSERT INTO big_copy SELECT i::INTEGER FROM generate_series(0, {}) AS t(i)",
            n_rows - 1
        ));

        let mut result =
            execute_sql_raw(db.conn, "SELECT i FROM big_copy ORDER BY i").expect("query failed");

        let chunk_count = ffi::duckdb_result_chunk_count(result) as usize;
        assert!(chunk_count > 1);

        let mut logical_types = vec![ffi::duckdb_column_logical_type(&mut result, 0)];
        let lt = logical_types[0];
        let type_id = ffi::duckdb_get_type_id(lt) as u32;

        let mut total_read = 0_usize;
        for chunk_idx in 0..chunk_count {
            let mut src_chunk = ffi::duckdb_result_get_chunk(result, chunk_idx as ffi::idx_t);
            let row_count = ffi::duckdb_data_chunk_get_size(src_chunk) as usize;

            let out_chunk = ffi::duckdb_create_data_chunk(logical_types.as_mut_ptr(), 1);
            let sel = create_identity_sel(row_count);

            let src_vec = ffi::duckdb_data_chunk_get_vector(src_chunk, 0);
            let dst_vec = ffi::duckdb_data_chunk_get_vector(out_chunk, 0);
            ffi::duckdb_vector_copy_sel(src_vec, dst_vec, sel, row_count as ffi::idx_t, 0, 0);
            ffi::duckdb_data_chunk_set_size(out_chunk, row_count as ffi::idx_t);

            ffi::duckdb_destroy_data_chunk(&mut src_chunk);
            ffi::duckdb_destroy_selection_vector(sel);

            for row_idx in 0..row_count {
                let val = read_typed_value(out_chunk, 0, row_idx, type_id, lt);
                let expected = (total_read + row_idx) as i32;
                assert_eq!(val, TestValue::I32(expected));
            }
            total_read += row_count;

            ffi::duckdb_destroy_data_chunk(&mut { out_chunk });
        }
        assert_eq!(total_read, n_rows);

        for lt in &mut logical_types {
            ffi::duckdb_destroy_logical_type(lt);
        }
        ffi::duckdb_destroy_result(&mut result);
    }
}
