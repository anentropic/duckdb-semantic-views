---
created: 2026-03-19T23:22:25.398Z
title: Investigate WASM build strategy
area: tooling
files:
  - build.rs
  - shim/shim.cpp
  - Cargo.toml
---

## Problem

DuckDB has a WASM build target (used by DuckDB-WASM / shell.duckdb.org), and it would be valuable to run the semantic views extension in the browser. However, the build strategy is unclear:

1. **As a loadable extension** — DuckDB supports WASM extensions, but the community extension registry build pipeline for hybrid Rust+C++ is already flagged as untested (see CE registry concern in STATE.md). WASM adds another dimension.
2. **As a custom DuckDB build** — since we already vendor the amalgamation (duckdb.cpp/duckdb.hpp via cc crate), we could compile the whole thing together into a single WASM binary. This avoids the extension loading mechanism entirely but produces a non-standard DuckDB distribution.

Key questions:
- Does DuckDB's WASM extension loading support Rust-compiled extensions?
- What's the emscripten story for the cc crate amalgamation compilation?
- Would a custom build be more practical given the C++ shim dependency?

## Solution

TBD — needs research into:
- DuckDB-WASM extension architecture (httpfs-style dynamic loading vs static linking)
- emscripten + Rust + cc crate compatibility
- Whether the parser extension hooks (C_STRUCT entry point) work in WASM context
- Size budget considerations (amalgamation is ~20MB uncompressed)
