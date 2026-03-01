// src/shim/shim.h
// extern "C" boundary between the C++ shim and Rust.
// Must be includable from both C++ (shim.cpp) and plain C.
#pragma once

#ifdef __cplusplus
extern "C" {
#endif

// Called from Rust init_extension to wire up C++ hooks.
// Phase 8: intentional no-op. Phases 10/11 add parser and pragma registration.
// db_instance_ptr is a pointer to a DuckDB DatabaseInstance; cast in later phases.
void semantic_views_register_shim(void* db_instance_ptr);

#ifdef __cplusplus
}
#endif
