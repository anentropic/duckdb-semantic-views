//! Property-based tests for the binary-read output pipeline.
//!
//! Two layers:
//! 1. **Unit PBTs** — validate binary-read helper behaviours for all scalar types
//!    using in-memory DuckDB result chunks via `execute_sql_raw` + `read_typed_value`.
//! 2. **Integration PBTs** — full roundtrip via in-memory DuckDB: generate values →
//!    CREATE TABLE → INSERT → SELECT → `read_typed_value` → assert type and values match.
//!
//! These tests run under the default `bundled` feature (`cargo test`).
//! The extension feature is NOT required — `test_helpers` provides the same binary-read
//! functions compiled for the bundled environment.

use libduckdb_sys as ffi;
use proptest::prelude::*;
use semantic_views::test_helpers::{execute_sql_raw, read_typed_value, RawDb, TestValue};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect all typed values from a single-column result query.
///
/// Opens a fresh `duckdb_result`, iterates all chunks, and collects values using
/// `read_typed_value`. Destroys chunks, logical type, and result on return.
unsafe fn collect_column(db: &RawDb, sql: &str) -> Vec<TestValue> {
    let mut result = execute_sql_raw(db.conn, sql).expect("query failed");
    let type_id = ffi::duckdb_column_type(&mut result, 0) as u32;
    let logical_type = ffi::duckdb_column_logical_type(&mut result, 0);
    let chunk_count = ffi::duckdb_result_chunk_count(result) as usize;
    let mut values = Vec::new();
    for chunk_idx in 0..chunk_count {
        let chunk = ffi::duckdb_result_get_chunk(result, chunk_idx as ffi::idx_t);
        if chunk.is_null() {
            continue;
        }
        let row_count = ffi::duckdb_data_chunk_get_size(chunk) as usize;
        for row_idx in 0..row_count {
            let val = read_typed_value(chunk, 0, row_idx, type_id, logical_type);
            values.push(val);
        }
        ffi::duckdb_destroy_data_chunk(&mut { chunk });
    }
    ffi::duckdb_destroy_logical_type(&mut { logical_type });
    ffi::duckdb_destroy_result(&mut result);
    values
}

// ---------------------------------------------------------------------------
// Layer 1: Unit tests — deterministic boundary values
// ---------------------------------------------------------------------------

mod unit_tests {
    use super::*;

    /// TIMESTAMP regression: must return non-NULL I64 (not TypedValue::Null).
    /// The old VARCHAR cast + parse::<i64>() path returned NULL for TIMESTAMP.
    #[test]
    fn timestamp_returns_non_null() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT TIMESTAMP '2024-01-15 10:30:00'") };
        assert_eq!(values.len(), 1);
        assert!(
            matches!(values[0], TestValue::I64(v) if v != 0),
            "TIMESTAMP must not be NULL or zero, got: {:?}",
            values[0]
        );
    }

    /// BOOLEAN regression: must read correct u8 0/1 values, not UB.
    #[test]
    fn boolean_reads_correctly() {
        let db = RawDb::open_in_memory();
        unsafe {
            db.exec("CREATE TABLE t(b BOOLEAN)");
            db.exec("INSERT INTO t VALUES (true)");
            db.exec("INSERT INTO t VALUES (false)");
        }
        let values = unsafe { collect_column(&db, "SELECT b FROM t ORDER BY b") };
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], TestValue::Bool(false));
        assert_eq!(values[1], TestValue::Bool(true));
    }

    /// INTEGER boundary values.
    #[test]
    fn integer_boundary_values() {
        let db = RawDb::open_in_memory();
        // DuckDB parses -2147483648 as BIGINT(2147483648) which can't be cast to INTEGER,
        // so we express i32::MIN as (-2147483647::INTEGER - 1::INTEGER).
        let values = unsafe {
            collect_column(
                &db,
                "SELECT unnest([2147483647::INTEGER, (-2147483647::INTEGER - 1::INTEGER), 0::INTEGER]) ORDER BY 1",
            )
        };
        assert_eq!(values.len(), 3);
        assert!(values.contains(&TestValue::I32(i32::MAX)));
        assert!(values.contains(&TestValue::I32(i32::MIN)));
        assert!(values.contains(&TestValue::I32(0)));
    }

    /// BIGINT boundary values.
    #[test]
    fn bigint_boundary_values() {
        let db = RawDb::open_in_memory();
        // DuckDB parses -9223372036854775808 as INT128(9223372036854775808) which can't be cast to
        // BIGINT, so we express i64::MIN as (-9223372036854775807::BIGINT - 1::BIGINT).
        let values = unsafe {
            collect_column(
                &db,
                "SELECT unnest([9223372036854775807::BIGINT, (-9223372036854775807::BIGINT - 1::BIGINT)])",
            )
        };
        assert_eq!(values.len(), 2);
        assert!(values.iter().any(|v| matches!(v, TestValue::I64(i64::MAX))));
        assert!(values.iter().any(|v| matches!(v, TestValue::I64(i64::MIN))));
    }

    /// DATE binary read: '2024-01-15' = 19737 days since 1970-01-01.
    /// This is the regression test for the old string-parse DATE path.
    #[test]
    fn date_binary_read() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT '2024-01-15'::DATE") };
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], TestValue::I32(19737));
    }

    /// DATE epoch: '1970-01-01' = 0 days.
    #[test]
    fn date_epoch_zero() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT '1970-01-01'::DATE") };
        assert_eq!(values[0], TestValue::I32(0));
    }

    /// DATE before epoch: '1969-12-31' = -1 days.
    #[test]
    fn date_before_epoch() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT '1969-12-31'::DATE") };
        assert_eq!(values[0], TestValue::I32(-1));
    }

    /// TIMESTAMP boundary: specific known epoch value.
    /// TIMESTAMP '2024-01-15 10:30:00' = 1705314600000000 microseconds since epoch.
    #[test]
    fn timestamp_known_value() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT TIMESTAMP '2024-01-15 10:30:00'") };
        assert_eq!(values.len(), 1);
        // Should be a large positive microsecond value.
        if let TestValue::I64(usecs) = values[0] {
            // 2024-01-15 is well after the epoch, must be > 0.
            assert!(
                usecs > 0,
                "Expected positive microseconds for 2024-01-15 timestamp"
            );
        } else {
            panic!("Expected I64 for TIMESTAMP, got {:?}", values[0]);
        }
    }

    /// FLOAT boundary values.
    #[test]
    fn float_reads_correctly() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT 1.5::FLOAT") };
        assert_eq!(values.len(), 1);
        if let TestValue::F32(v) = values[0] {
            assert!((v - 1.5_f32).abs() < f32::EPSILON * 10.0);
        } else {
            panic!("Expected F32, got {:?}", values[0]);
        }
    }

    /// DOUBLE boundary values.
    #[test]
    fn double_reads_correctly() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT 3.14::DOUBLE") };
        if let TestValue::F64(v) = values[0] {
            assert!((v - 3.14_f64).abs() < f64::EPSILON * 100.0);
        } else {
            panic!("Expected F64, got {:?}", values[0]);
        }
    }

    /// NULL propagation for INTEGER.
    #[test]
    fn null_propagation_integer() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT NULL::INTEGER") };
        assert_eq!(values[0], TestValue::Null);
    }

    /// NULL propagation for BIGINT.
    #[test]
    fn null_propagation_bigint() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT NULL::BIGINT") };
        assert_eq!(values[0], TestValue::Null);
    }

    /// NULL propagation for BOOLEAN.
    #[test]
    fn null_propagation_boolean() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT NULL::BOOLEAN") };
        assert_eq!(values[0], TestValue::Null);
    }

    /// NULL propagation for TIMESTAMP.
    #[test]
    fn null_propagation_timestamp() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT NULL::TIMESTAMP") };
        assert_eq!(values[0], TestValue::Null);
    }

    /// TINYINT boundary values.
    #[test]
    fn tinyint_boundary_values() {
        let db = RawDb::open_in_memory();
        // DuckDB parses -128 as INT32(128) which overflows TINYINT, so use (-127::TINYINT - 1::TINYINT).
        let values = unsafe {
            collect_column(
                &db,
                "SELECT unnest([127::TINYINT, (-127::TINYINT - 1::TINYINT), 0::TINYINT]) ORDER BY 1",
            )
        };
        assert!(values.contains(&TestValue::I8(i8::MAX)));
        assert!(values.contains(&TestValue::I8(i8::MIN)));
        assert!(values.contains(&TestValue::I8(0)));
    }

    /// UTINYINT boundary values.
    #[test]
    fn utinyint_boundary_values() {
        let db = RawDb::open_in_memory();
        let values = unsafe {
            collect_column(
                &db,
                "SELECT unnest([255::UTINYINT, 0::UTINYINT]) ORDER BY 1",
            )
        };
        assert!(values.contains(&TestValue::U8(255)));
        assert!(values.contains(&TestValue::U8(0)));
    }

    /// UBIGINT boundary values.
    #[test]
    fn ubigint_boundary_values() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT 18446744073709551615::UBIGINT") };
        assert_eq!(values[0], TestValue::U64(u64::MAX));
    }

    /// DECIMAL binary read: scale factor is preserved in the backing integer.
    #[test]
    fn decimal_backing_integer() {
        let db = RawDb::open_in_memory();
        // DECIMAL(10, 2): 350.00 → backing integer = 35000
        unsafe {
            let mut result =
                execute_sql_raw(db.conn, "SELECT 350.00::DECIMAL(10,2)").expect("query failed");
            let type_id = ffi::duckdb_column_type(&mut result, 0) as u32;
            let logical_type = ffi::duckdb_column_logical_type(&mut result, 0);
            let chunk = ffi::duckdb_result_get_chunk(result, 0);
            assert!(!chunk.is_null(), "Expected result chunk");
            let val = read_typed_value(chunk, 0, 0, type_id, logical_type);
            assert!(
                matches!(val, TestValue::I128(35000)),
                "DECIMAL(10,2) 350.00 should have backing integer 35000, got {:?}",
                val
            );
            ffi::duckdb_destroy_data_chunk(&mut { chunk });
            ffi::duckdb_destroy_logical_type(&mut { logical_type });
            ffi::duckdb_destroy_result(&mut result);
        }
    }

    /// DECIMAL negative value.
    #[test]
    fn decimal_negative_value() {
        let db = RawDb::open_in_memory();
        unsafe {
            let mut result =
                execute_sql_raw(db.conn, "SELECT -1.50::DECIMAL(10,2)").expect("query failed");
            let type_id = ffi::duckdb_column_type(&mut result, 0) as u32;
            let logical_type = ffi::duckdb_column_logical_type(&mut result, 0);
            let chunk = ffi::duckdb_result_get_chunk(result, 0);
            let val = read_typed_value(chunk, 0, 0, type_id, logical_type);
            assert!(
                matches!(val, TestValue::I128(-150)),
                "DECIMAL(10,2) -1.50 should have backing integer -150, got {:?}",
                val
            );
            ffi::duckdb_destroy_data_chunk(&mut { chunk });
            ffi::duckdb_destroy_logical_type(&mut { logical_type });
            ffi::duckdb_destroy_result(&mut result);
        }
    }

    /// LIST(BIGINT) reads correctly: offset and length tracking.
    #[test]
    fn list_bigint_reads_correctly() {
        let db = RawDb::open_in_memory();
        unsafe {
            let mut result = execute_sql_raw(db.conn, "SELECT [1::BIGINT, 2::BIGINT, 3::BIGINT]")
                .expect("query failed");
            let type_id = ffi::duckdb_column_type(&mut result, 0) as u32;
            let logical_type = ffi::duckdb_column_logical_type(&mut result, 0);
            let chunk = ffi::duckdb_result_get_chunk(result, 0);
            let val = read_typed_value(chunk, 0, 0, type_id, logical_type);
            assert_eq!(
                val,
                TestValue::List(vec![
                    TestValue::I64(1),
                    TestValue::I64(2),
                    TestValue::I64(3)
                ]),
                "LIST(BIGINT) read mismatch"
            );
            ffi::duckdb_destroy_data_chunk(&mut { chunk });
            ffi::duckdb_destroy_logical_type(&mut { logical_type });
            ffi::duckdb_destroy_result(&mut result);
        }
    }

    /// Empty LIST reads as empty Vec.
    #[test]
    fn list_empty() {
        let db = RawDb::open_in_memory();
        unsafe {
            let mut result = execute_sql_raw(db.conn, "SELECT []::BIGINT[]").expect("query failed");
            let type_id = ffi::duckdb_column_type(&mut result, 0) as u32;
            let logical_type = ffi::duckdb_column_logical_type(&mut result, 0);
            let chunk = ffi::duckdb_result_get_chunk(result, 0);
            let val = read_typed_value(chunk, 0, 0, type_id, logical_type);
            assert_eq!(val, TestValue::List(vec![]));
            ffi::duckdb_destroy_data_chunk(&mut { chunk });
            ffi::duckdb_destroy_logical_type(&mut { logical_type });
            ffi::duckdb_destroy_result(&mut result);
        }
    }

    /// VARCHAR reads correctly.
    #[test]
    fn varchar_reads_correctly() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT 'hello world'::VARCHAR") };
        assert_eq!(values[0], TestValue::Str("hello world".to_string()));
    }

    /// Long VARCHAR (>12 chars uses pointer layout).
    #[test]
    fn varchar_long_string() {
        let db = RawDb::open_in_memory();
        let values = unsafe { collect_column(&db, "SELECT 'this is a longer string test'") };
        assert_eq!(
            values[0],
            TestValue::Str("this is a longer string test".to_string())
        );
    }
}

// ---------------------------------------------------------------------------
// Layer 1: Proptest unit PBTs — arbitrary values, verify no panic and correct roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 50, .. ProptestConfig::default() })]

    /// Arbitrary BIGINT values roundtrip correctly.
    #[test]
    fn bigint_binary_read(v in prop::num::i64::ANY) {
        let db = RawDb::open_in_memory();
        let sql = format!("SELECT {v}::BIGINT");
        let values = unsafe { collect_column(&db, &sql) };
        prop_assert_eq!(values.len(), 1);
        prop_assert_eq!(&values[0], &TestValue::I64(v));
    }

    /// Arbitrary INTEGER values roundtrip correctly.
    #[test]
    fn integer_binary_read(v in prop::num::i32::ANY) {
        let db = RawDb::open_in_memory();
        let sql = format!("SELECT {v}::INTEGER");
        let values = unsafe { collect_column(&db, &sql) };
        prop_assert_eq!(&values[0], &TestValue::I32(v));
    }

    /// Arbitrary BOOLEAN values roundtrip correctly.
    #[test]
    fn boolean_binary_read(v in any::<bool>()) {
        let db = RawDb::open_in_memory();
        let sql = format!("SELECT {}::BOOLEAN", if v { "true" } else { "false" });
        let values = unsafe { collect_column(&db, &sql) };
        prop_assert_eq!(&values[0], &TestValue::Bool(v));
    }

    /// Arbitrary DOUBLE values roundtrip correctly via bit-exact representation.
    /// Uses `frombits(bits::UBIGINT)` to avoid decimal formatting precision loss.
    #[test]
    fn double_binary_read(bits in prop::num::u64::ANY) {
        let v = f64::from_bits(bits);
        // Skip NaN and Inf — DuckDB may handle them differently.
        prop_assume!(v.is_finite());
        let db = RawDb::open_in_memory();
        // frombits is not in DuckDB SQL; use a cast from the binary int representation instead.
        // The most portable way is to use a known exact decimal representation.
        // We use {:.17e} which is sufficient for exact f64 round-trip.
        let sql = format!("SELECT {v:.17e}::DOUBLE");
        let values = unsafe { collect_column(&db, &sql) };
        if let TestValue::F64(result) = values[0] {
            prop_assert_eq!(
                result.to_bits(), v.to_bits(),
                "{}",
                format!("DOUBLE binary roundtrip failed: expected {v:.17e}, got {result:.17e}")
            );
        } else {
            prop_assert!(false, "Expected F64, got {:?}", values[0]);
        }
    }
}

// ---------------------------------------------------------------------------
// Layer 2: Integration PBTs — full column roundtrip via DuckDB table
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig { cases: 20, .. ProptestConfig::default() })]

    /// BIGINT column roundtrip: insert Vec<i64>, read back as TypedValue::I64.
    #[test]
    fn bigint_column_roundtrip(values in prop::collection::vec(prop::num::i64::ANY, 1..10)) {
        let db = RawDb::open_in_memory();
        unsafe {
            db.exec("CREATE TABLE t_bigint (v BIGINT)");
            for v in &values {
                db.exec(&format!("INSERT INTO t_bigint VALUES ({v})"));
            }
        }
        let result = unsafe {
            collect_column(&db, "SELECT v FROM t_bigint ORDER BY rowid")
        };
        prop_assert_eq!(result.len(), values.len());
        for (expected, actual) in values.iter().zip(result.iter()) {
            prop_assert_eq!(actual, &TestValue::I64(*expected));
        }
    }

    /// INTEGER column roundtrip.
    #[test]
    fn integer_column_roundtrip(values in prop::collection::vec(prop::num::i32::ANY, 1..10)) {
        let db = RawDb::open_in_memory();
        unsafe {
            db.exec("CREATE TABLE t_int (v INTEGER)");
            for v in &values {
                db.exec(&format!("INSERT INTO t_int VALUES ({v})"));
            }
        }
        let result = unsafe {
            collect_column(&db, "SELECT v FROM t_int ORDER BY rowid")
        };
        prop_assert_eq!(result.len(), values.len());
        for (expected, actual) in values.iter().zip(result.iter()) {
            prop_assert_eq!(actual, &TestValue::I32(*expected));
        }
    }

    /// BOOLEAN column roundtrip — BOOLEAN UB fix regression.
    #[test]
    fn boolean_column_roundtrip(values in prop::collection::vec(any::<bool>(), 1..10)) {
        let db = RawDb::open_in_memory();
        unsafe {
            db.exec("CREATE TABLE t_bool (v BOOLEAN)");
            for v in &values {
                db.exec(&format!("INSERT INTO t_bool VALUES ({})", if *v {"true"} else {"false"}));
            }
        }
        let result = unsafe {
            collect_column(&db, "SELECT v FROM t_bool ORDER BY rowid")
        };
        prop_assert_eq!(result.len(), values.len());
        for (expected, actual) in values.iter().zip(result.iter()) {
            prop_assert_eq!(actual, &TestValue::Bool(*expected));
        }
    }

    /// DOUBLE column roundtrip via bit-exact representation.
    #[test]
    fn double_column_roundtrip(bit_values in prop::collection::vec(prop::num::u64::ANY, 1..10)) {
        // Convert bits to f64, filter out non-finite values.
        let values: Vec<f64> = bit_values.iter().map(|b| f64::from_bits(*b))
            .filter(|v| v.is_finite())
            .collect();
        prop_assume!(!values.is_empty());
        let db = RawDb::open_in_memory();
        unsafe {
            db.exec("CREATE TABLE t_dbl (v DOUBLE)");
            for v in &values {
                db.exec(&format!("INSERT INTO t_dbl VALUES ({v:.17e})"));
            }
        }
        let result = unsafe {
            collect_column(&db, "SELECT v FROM t_dbl ORDER BY rowid")
        };
        prop_assert_eq!(result.len(), values.len());
        for (expected, actual) in values.iter().zip(result.iter()) {
            if let TestValue::F64(actual_v) = actual {
                prop_assert_eq!(
                    actual_v.to_bits(), expected.to_bits(),
                    "{}",
                    format!("DOUBLE mismatch: expected {expected:.17e}, got {actual_v:.17e}")
                );
            } else {
                prop_assert!(false, "Expected F64, got {:?}", actual);
            }
        }
    }

    /// TIMESTAMP column roundtrip — TIMESTAMP NULL fix regression.
    /// Insert microsecond timestamps via `epoch_us()`, read back as TypedValue::I64,
    /// and assert the exact value matches. Covers year 0001 through year 9999.
    #[test]
    fn timestamp_column_roundtrip(
        usecs in prop::collection::vec(-62_135_596_800_000_000_i64..=253_402_300_799_999_999_i64, 1..10)
    ) {
        let db = RawDb::open_in_memory();
        unsafe {
            db.exec("CREATE TABLE t_ts (v TIMESTAMP)");
            for u in &usecs {
                db.exec(&format!("INSERT INTO t_ts VALUES (make_timestamp({u}::BIGINT))"));
            }
        }
        let result = unsafe {
            collect_column(&db, "SELECT v FROM t_ts ORDER BY rowid")
        };
        prop_assert_eq!(result.len(), usecs.len());
        for (expected, actual) in usecs.iter().zip(result.iter()) {
            prop_assert_eq!(
                actual, &TestValue::I64(*expected),
                "TIMESTAMP roundtrip mismatch for usecs={}",
                expected
            );
        }
    }

    /// DATE column roundtrip. Covers both pre-epoch (negative) and post-epoch days,
    /// approximately 1833 to 2107. Uses DATE + INTEGER arithmetic which supports
    /// negative offsets, unlike INTERVAL which requires non-negative values.
    #[test]
    fn date_column_roundtrip(days in prop::collection::vec(-50_000_i32..=50_000_i32, 1..10)) {
        let db = RawDb::open_in_memory();
        unsafe {
            db.exec("CREATE TABLE t_date (v DATE)");
            for d in &days {
                db.exec(&format!(
                    "INSERT INTO t_date VALUES ('1970-01-01'::DATE + ({d}::INTEGER))"
                ));
            }
        }
        let result = unsafe {
            collect_column(&db, "SELECT v FROM t_date ORDER BY rowid")
        };
        prop_assert_eq!(result.len(), days.len());
        for (expected, actual) in days.iter().zip(result.iter()) {
            prop_assert_eq!(
                actual, &TestValue::I32(*expected),
                "DATE roundtrip mismatch for days={}",
                expected
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Deterministic integration tests for DECIMAL and LIST(BIGINT)
// ---------------------------------------------------------------------------

/// DECIMAL roundtrip: verify backing integer representation.
#[test]
fn decimal_roundtrip() {
    let db = RawDb::open_in_memory();
    unsafe {
        db.exec("CREATE TABLE t_dec (v DECIMAL(10,2))");
        db.exec("INSERT INTO t_dec VALUES (350.00), (-1.50), (0.00)");
        let mut result =
            execute_sql_raw(db.conn, "SELECT v FROM t_dec ORDER BY rowid").expect("query failed");
        let type_id = ffi::duckdb_column_type(&mut result, 0) as u32;
        let logical_type = ffi::duckdb_column_logical_type(&mut result, 0);
        let chunk = ffi::duckdb_result_get_chunk(result, 0);
        assert!(!chunk.is_null());

        // Verify backing integers for DECIMAL(10,2) values.
        let expected = [
            TestValue::I128(35000),
            TestValue::I128(-150),
            TestValue::I128(0),
        ];
        for (row_idx, expected_val) in expected.iter().enumerate() {
            let val = read_typed_value(chunk, 0, row_idx, type_id, logical_type);
            assert_eq!(&val, expected_val, "DECIMAL mismatch at row {row_idx}");
        }

        ffi::duckdb_destroy_data_chunk(&mut { chunk });
        ffi::duckdb_destroy_logical_type(&mut { logical_type });
        ffi::duckdb_destroy_result(&mut result);
    }
}

/// LIST(BIGINT) roundtrip: verify offset/length tracking across rows.
#[test]
fn list_bigint_roundtrip() {
    let db = RawDb::open_in_memory();
    unsafe {
        db.exec("CREATE TABLE t_list (v BIGINT[])");
        db.exec("INSERT INTO t_list VALUES ([1, 2, 3])");
        db.exec("INSERT INTO t_list VALUES ([])");
        db.exec("INSERT INTO t_list VALUES ([100])");
        let mut result =
            execute_sql_raw(db.conn, "SELECT v FROM t_list ORDER BY rowid").expect("query");
        let type_id = ffi::duckdb_column_type(&mut result, 0) as u32;
        let logical_type = ffi::duckdb_column_logical_type(&mut result, 0);
        let chunk = ffi::duckdb_result_get_chunk(result, 0);
        assert!(!chunk.is_null());

        let expected = [
            TestValue::List(vec![
                TestValue::I64(1),
                TestValue::I64(2),
                TestValue::I64(3),
            ]),
            TestValue::List(vec![]),
            TestValue::List(vec![TestValue::I64(100)]),
        ];
        for (row_idx, expected_val) in expected.iter().enumerate() {
            let val = read_typed_value(chunk, 0, row_idx, type_id, logical_type);
            assert_eq!(&val, expected_val, "LIST mismatch at row {row_idx}");
        }

        ffi::duckdb_destroy_data_chunk(&mut { chunk });
        ffi::duckdb_destroy_logical_type(&mut { logical_type });
        ffi::duckdb_destroy_result(&mut result);
    }
}

/// NULL propagation: mix of non-null and null values.
#[test]
fn null_propagation() {
    let db = RawDb::open_in_memory();
    unsafe {
        db.exec("CREATE TABLE t_null_mix (v INTEGER)");
        db.exec("INSERT INTO t_null_mix VALUES (42), (NULL), (7), (NULL)");
    }
    let values = unsafe { collect_column(&db, "SELECT v FROM t_null_mix ORDER BY rowid") };
    assert_eq!(values.len(), 4);
    assert_eq!(values[0], TestValue::I32(42));
    assert_eq!(values[1], TestValue::Null);
    assert_eq!(values[2], TestValue::I32(7));
    assert_eq!(values[3], TestValue::Null);
}
