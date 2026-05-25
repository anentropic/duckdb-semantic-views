//! Structural guard: `init_extension` must NOT call `duckdb_connect`.
//!
//! Phase 65 (v0.10.0) — ROADMAP success criterion 4.
//!
//! This test parses `src/lib.rs` and walks the syntax tree to confirm that
//! the `init_extension` function body does not contain a call to
//! `duckdb_connect`. Plan 06 retired the H1 `catalog_conn` allocation; Plan 05
//! retired the H2 `query_conn` allocation. Together they eliminated the
//! long-lived extension-owned `duckdb_connection` handles whose
//! `shared_ptr<DatabaseInstance>` references kept the underlying DuckDB
//! `Database` alive past the caller's `close()`, causing the in-process
//! RW→RO reopen busy-spin in `DBInstanceCache::GetInstanceInternal`
//! (LIFE-01 root cause; see
//! `.planning/phases/65-overridecontext-connection-teardown/65-01-SPIKES.md`
//! for the lldb evidence).
//!
//! The new architecture opens connections per-call from inside bind /
//! exec callbacks (`Connection probe(*context.db)` on the C++ side,
//! reinterpret_cast bridged to Rust). Re-introducing a long-lived
//! `duckdb_connection` in `init_extension` would silently re-create the
//! LIFE-01 hang. This test fails CI if anyone does so.
//!
//! ## Known limitation (documented per D-22 bounded scope)
//!
//! The visitor only matches the **last** segment of a call expression's
//! path against the literal identifier `duckdb_connect`. That catches the
//! common shapes:
//!   - `duckdb_connect(...)`
//!   - `ffi::duckdb_connect(...)`
//!   - `libduckdb_sys::duckdb_connect(...)`
//!
//! It does NOT catch aliasing via `use … as`:
//!
//!   ```rust,ignore
//!   use ffi::duckdb_connect as my_connect;
//!   my_connect(db, &mut conn);            // NOT detected
//!   ```
//!
//! Resolving the use-graph would require name resolution beyond what a
//! pure syntactic scan can do. Per D-22 (bounded scope with signal
//! surfacing) we accept this trade-off: anyone deliberately aliasing
//! `duckdb_connect` to evade the guard is consciously circumventing it,
//! which would show up at code review. The honest-mistake re-introduction
//! path is the call shape we DO catch.
//!
//! ## What this test does NOT cover (by design)
//!
//! - Helper modules outside `init_extension`. The crate's `RawDb` test
//!   helper at `src/lib.rs:226-277` legitimately calls `duckdb_connect`
//!   from inside `RawDb::open_in_memory()`; that's NOT a long-lived
//!   handle in `init_extension` and is scoped to test fixtures.
//! - C++ side `Connection probe(*context.db)` constructions inside
//!   bind / exec callbacks. Those ARE the correct per-call shape and
//!   live in `cpp/src/shim.cpp`.
//! - The other long-lived-handle candidates (`duckdb_open`,
//!   `duckdb_open_ext`). Those would create a whole new DB, not just
//!   a connection — much more obvious in code review than a stray
//!   `duckdb_connect`. Worth extending if a regression appears.

use syn::visit::Visit;
use syn::{ExprCall, ItemFn};

/// Walks `init_extension`'s body looking for any call expression whose
/// last path segment is the identifier `duckdb_connect`.
struct Finder {
    in_init_extension: bool,
    found_call_sites: Vec<String>,
}

impl<'ast> Visit<'ast> for Finder {
    fn visit_item_fn(&mut self, f: &'ast ItemFn) {
        let was = self.in_init_extension;
        if f.sig.ident == "init_extension" {
            self.in_init_extension = true;
        }
        syn::visit::visit_item_fn(self, f);
        self.in_init_extension = was;
    }

    fn visit_expr_call(&mut self, c: &'ast ExprCall) {
        if self.in_init_extension {
            if let syn::Expr::Path(p) = &*c.func {
                if let Some(last) = p.path.segments.last() {
                    if last.ident == "duckdb_connect" {
                        // Capture the full path for the assert message.
                        let segs: Vec<String> = p
                            .path
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect();
                        self.found_call_sites.push(segs.join("::"));
                    }
                }
            }
        }
        syn::visit::visit_expr_call(self, c);
    }
}

#[test]
fn init_extension_has_no_duckdb_connect_call() {
    let src = std::fs::read_to_string("src/lib.rs").expect("read src/lib.rs");
    let file: syn::File = syn::parse_str(&src).expect("parse src/lib.rs");

    let mut f = Finder {
        in_init_extension: false,
        found_call_sites: Vec::new(),
    };
    f.visit_file(&file);

    assert!(
        f.found_call_sites.is_empty(),
        "init_extension contains a duckdb_connect call site (found: {:?}). \
         Phase 65 (v0.10.0) retired long-lived extension-owned \
         duckdb_connection handles (H1 catalog_conn + H2 query_conn) to \
         resolve the in-process RW→RO reopen hang (LIFE-01). If a new \
         connection is genuinely needed, open it via a per-call \
         Connection(*context.db) inside a bind / exec callback on the \
         C++ side instead. See \
         .planning/phases/65-overridecontext-connection-teardown/ for \
         the full record.",
        f.found_call_sites
    );
}
