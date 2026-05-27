---
title: Address Copilot PR #35 review items
status: in-progress
created: 2026-05-27
---

# Quick Task 260527-p9t — Address Copilot PR #35 review items

Five concerns raised in Copilot's 2026-05-27 16:51 review on PR #35
(commit fe9d59d). Each addressed as an atomic commit.

## WR-05 — `CatalogReader` lifetime soundness

- File: `src/catalog.rs`
- Add a lifetime parameter `CatalogReader<'a>` carrying
  `PhantomData<&'a BorrowedConnection>` so the borrow scope is tracked.
- Remove `#[derive(Clone, Copy)]` and the `unsafe impl Send/Sync`.
- `BorrowedConnection` already wraps a raw pointer and is neither `Send`
  nor `Sync`, so `&'a BorrowedConnection` is neither either — the
  `PhantomData` makes `CatalogReader` inherit those !Send/!Sync semantics
  without an `unsafe impl` lying about it.
- All ~17 call sites use `let reader = CatalogReader::new(&borrowed, ...)`
  in the same scope as the stack-owned `Connection probe` — lifetime is
  inferred, no call-site edits required.
- Tests: `cargo test` (unit + structural) and one sqllogictest to
  exercise the catalog read path.

## WR-06 — Close cursors+master in `test_concurrent_reads_per_call_conn.py`

- File: `test/integration/test_concurrent_reads_per_call_conn.py`
- Wrap the post-`open_master` body in `try/finally`; close cursors then
  master before the `TemporaryDirectory()` context exits.

## WR-07 — Close cursors+master in `test_concurrent_writes_per_call_conn.py`

- File: `test/integration/test_concurrent_writes_per_call_conn.py`
- Same fix as WR-06.

## WR-08 — Fix per-row/per-chunk comment in `cpp/src/shim.cpp`

- File: `cpp/src/shim.cpp` around line 1925
- Comment currently says "Per-row Connection construction (rather than
  per-chunk)" but the actual code constructs `Connection probe` outside
  the row loop, i.e. **per-chunk**. Rewrite the comment to describe
  per-chunk behaviour (uniformity with TF migrations; one Connection per
  exec call; scalar usage in practice is one or two rows so cost is
  immaterial).

## WR-09 — Wire new concurrency tests into `Justfile`

- File: `Justfile`
- Extend `test-concurrent` to also run
  `test_concurrent_reads_per_call_conn.py` and
  `test_concurrent_writes_per_call_conn.py`, so `test-all` picks them up.

## Commit plan

One atomic commit per WR item, prefixed `fix(quick): WR-NN`.
