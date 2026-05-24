//! Process-local type-inference cache for semantic-view read paths.
//!
//! # Why this module exists (Phase 65 Plan 05, D-16 / D-17)
//!
//! v0.7.1 ran type-inference probes (`SELECT ... LIMIT 0` on the expanded
//! SQL, then `typeof()` lookups on the resulting columns) at CREATE-time and
//! persisted the inferred column types into the JSON definition stored in
//! `semantic_layer._definitions`. Plan 03 retired that CREATE-time machinery
//! because the catalog write happens on the caller's connection inside the
//! `parser_override` rewrite — there is no safe place to run a
//! `SELECT ... LIMIT 0` probe at parse time (would re-enter the planner).
//!
//! Phase 65 Plan 05 restores the user-visible "`data_type` populated" behavior
//! by deferring the same probe to read-side bind time. The probe now runs
//! on a per-call `Connection(*context.db)` opened inside the bind callback
//! (see `cpp/src/shim.cpp::sv_<name>_bind`). To avoid re-probing on every
//! `DESCRIBE` / `SHOW COLUMNS` / `SHOW DIMENSIONS` invocation, results are
//! memoised here, keyed on `(view_name, schema_fingerprint)`.
//!
//! # Cache shape
//!
//! Process-local `OnceLock<RwLock<HashMap<(String, u64), Arc<InferredTypes>>>>`.
//! The fingerprint is a stable hash of the relevant `SemanticViewDefinition`
//! fields (tables, dimensions, metrics, facts). ALTER produces a new JSON
//! definition → new fingerprint → cache miss on next probe → re-infer.
//! Old entries become unreachable but not evicted (see anti-pattern below).
//!
//! # Anti-pattern: NOT a bounded LRU
//!
//! TECH-DEBT 20 (Phase 62) documents why bounded LRUs are an anti-pattern
//! for the semantic-views extension: silent eviction produces correctness
//! errors that are extremely hard to debug. Phase 62 retired the 16-entry
//! `LruCache<DbToken, OverrideContext>` for the same reason.
//!
//! The keyspace here is bounded by
//! `unique_views × distinct_definitions_across_session` per process. Each
//! entry is tiny (a `Vec<(String, String)>` of column names + type names).
//! For realistic workloads (< 10,000 distinct view definitions per process
//! lifetime) total memory is well under 10 MiB — acceptable per RESEARCH §6.2.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock, RwLock};

/// Inferred column types for a single semantic view, as `(name, type_name)`
/// pairs (`DuckDB`'s `typeof()` strings — e.g. `"BIGINT"`, `"VARCHAR"`,
/// `"DECIMAL(18,2)"`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InferredTypes {
    pub column_types: Vec<(String, String)>,
}

impl InferredTypes {
    /// Look up the inferred type for a column by name, case-insensitive.
    /// Returns `None` if the column is not present in the cached probe.
    #[must_use]
    pub fn lookup(&self, column_name: &str) -> Option<&str> {
        self.column_types
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(column_name))
            .map(|(_, t)| t.as_str())
    }
}

/// Key: (`view_name`, `schema_fingerprint`). Value: shared `InferredTypes`.
type CacheMap = HashMap<(String, u64), Arc<InferredTypes>>;

static TYPE_CACHE: OnceLock<RwLock<CacheMap>> = OnceLock::new();

fn cache() -> &'static RwLock<CacheMap> {
    TYPE_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Stable fingerprint of an opaque payload (typically the JSON definition
/// string or a tuple of its load-bearing fields). The implementation uses
/// `std::collections::hash_map::DefaultHasher` which is process-stable but
/// NOT version-stable — that's fine because the cache itself is process-local.
#[must_use]
pub fn fingerprint(payload: &str) -> u64 {
    let mut h = DefaultHasher::new();
    payload.hash(&mut h);
    h.finish()
}

/// Atomic look-up-or-compute. The `probe` closure runs at most once per
/// `(view_name, fingerprint)` pair across the process's lifetime — concurrent
/// callers either find an existing entry in the read lock OR serialise on the
/// write lock and the first writer fills the entry.
///
/// # Errors
///
/// Returns the error from `probe` verbatim on cache miss.
pub fn lookup_or_probe<F>(
    view_name: &str,
    fingerprint: u64,
    probe: F,
) -> Result<Arc<InferredTypes>, String>
where
    F: FnOnce() -> Result<InferredTypes, String>,
{
    let key = (view_name.to_string(), fingerprint);
    {
        let guard = cache()
            .read()
            .map_err(|e| format!("type cache poisoned (read): {e}"))?;
        if let Some(hit) = guard.get(&key) {
            return Ok(Arc::clone(hit));
        }
    }
    // Cache miss — acquire the write lock and re-check (another writer may
    // have filled the entry between the read lock release and our acquire).
    let mut guard = cache()
        .write()
        .map_err(|e| format!("type cache poisoned (write): {e}"))?;
    if let Some(hit) = guard.get(&key) {
        return Ok(Arc::clone(hit));
    }
    let inferred = probe()?;
    let arc = Arc::new(inferred);
    guard.insert(key, Arc::clone(&arc));
    Ok(arc)
}

/// Test-only: clear the cache. Used by unit tests to isolate cases.
#[cfg(test)]
pub fn clear() {
    if let Some(c) = TYPE_CACHE.get() {
        if let Ok(mut g) = c.write() {
            g.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn fingerprint_is_stable_within_process() {
        let a = fingerprint("definition-json");
        let b = fingerprint("definition-json");
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_differs_on_change() {
        let a = fingerprint("a");
        let b = fingerprint("b");
        assert_ne!(a, b);
    }

    #[test]
    fn cold_probe_runs_then_caches() {
        clear();
        let calls = AtomicUsize::new(0);
        let fp = fingerprint("v1-def");
        let r1 = lookup_or_probe("vt_cold_probe_view", fp, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(InferredTypes {
                column_types: vec![("id".to_string(), "BIGINT".to_string())],
            })
        })
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let r2 = lookup_or_probe("vt_cold_probe_view", fp, || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(InferredTypes::default())
        })
        .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second call must hit cache"
        );
        assert_eq!(r1.column_types, r2.column_types);
    }

    #[test]
    fn different_fingerprint_invalidates() {
        clear();
        let view = "vt_invalidate_view";
        let _r1 = lookup_or_probe("vt_invalidate_view", fingerprint("v1"), || {
            Ok(InferredTypes {
                column_types: vec![("a".to_string(), "VARCHAR".to_string())],
            })
        })
        .unwrap();
        let r2 = lookup_or_probe(view, fingerprint("v2"), || {
            Ok(InferredTypes {
                column_types: vec![("a".to_string(), "BIGINT".to_string())],
            })
        })
        .unwrap();
        assert_eq!(r2.column_types[0].1, "BIGINT");
    }

    #[test]
    fn lookup_case_insensitive() {
        let t = InferredTypes {
            column_types: vec![("Total_Sales".to_string(), "DECIMAL(18,2)".to_string())],
        };
        assert_eq!(t.lookup("total_sales"), Some("DECIMAL(18,2)"));
        assert_eq!(t.lookup("TOTAL_SALES"), Some("DECIMAL(18,2)"));
        assert_eq!(t.lookup("missing"), None);
    }

    #[test]
    fn probe_error_propagates() {
        clear();
        let r: Result<_, String> = lookup_or_probe("vt_err_view", fingerprint("e"), || {
            Err("probe failed".to_string())
        });
        assert_eq!(r.unwrap_err(), "probe failed");
        // Failed probe must NOT cache — next call should re-attempt.
        let calls = AtomicUsize::new(0);
        let _ = lookup_or_probe("vt_err_view", fingerprint("e"), || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(InferredTypes::default())
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
