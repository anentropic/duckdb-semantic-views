---
title: Address Copilot PR #35 review items
status: complete
completed: 2026-05-27
---

# Quick Task 260527-p9t — SUMMARY

All five items from Copilot's 2026-05-27 16:51 review on PR #35 addressed
as atomic commits on `milestone/v0.10.0`:

| WR  | Commit  | File(s)                                                  | Fix |
| --- | ------- | -------------------------------------------------------- | --- |
| 05  | 6ad639d | `src/catalog.rs`                                         | `CatalogReader<'a>` lifetime parameter via `PhantomData<&'a BorrowedConnection>`; remove `Clone/Copy` derive and `unsafe impl Send/Sync`. Reader now cannot escape the borrow scope of the stack `Connection probe`. |
| 06  | 00067dc | `test/integration/test_concurrent_reads_per_call_conn.py` | Wrap body in `try/finally`; close cursors then master before `TemporaryDirectory()` exits. |
| 07  | d5eaf69 | `test/integration/test_concurrent_writes_per_call_conn.py` | Same `try/finally` cleanup pattern as WR-06. |
| 08  | e175e8b | `cpp/src/shim.cpp`                                       | Rewrite stale "Per-row Connection construction" comment to describe actual per-chunk behaviour for `sv_get_ddl_exec` and `sv_read_yaml_from_semantic_view_exec`. |
| 09  | 541a5d0 | `Justfile`                                               | Extend `test-concurrent` to also run the two new per-call Connection regression scripts so `test-all` picks them up. |

## Verification

- `cargo nextest run` — 974/974 pass (after WR-05).
- `cargo build --features extension` — clean.
- `just test-concurrent` — all three scripts pass (80 reads in 0.02s,
  80 writes in 0.03s, single-name CREATE race still serialised on PK).

## Out of scope (intentionally not in this task)

- `cargo clippy --features extension -- -D warnings` reports many
  pre-existing pedantic errors on the extension feature; `just lint`
  only runs `cargo clippy` on default features, which passes. No
  regression vs baseline.
- Clangd LSP diagnostics for `cpp/src/shim.cpp` ("duckdb.hpp file not
  found") are an IDE-side missing-include-path issue, not a build
  break — the actual build/test pipeline produces a working extension.
