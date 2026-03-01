// build.rs
// Cargo build script — compiles the C++ shim when building the loadable extension.
//
// Design: Only the `extension` feature triggers C++ compilation. During `cargo test`
// (default/bundled feature), this script exits immediately — no C++ toolchain required.
//
// Symbol visibility: restricts the exported symbols of the cdylib to the two DuckDB
// entry points on Linux and macOS. Without this, Rust stdlib symbols leak into the
// extension binary.

fn main() {
    // Only compile the C++ shim when building the loadable extension binary.
    // CARGO_FEATURE_EXTENSION is set by Cargo when `--features extension` is passed.
    // During `cargo test` (uses default/bundled feature), this block is skipped.
    if std::env::var("CARGO_FEATURE_EXTENSION").is_err() {
        return;
    }

    cc::Build::new()
        .cpp(true) // C++ mode (uses CXX, not CC)
        .file("src/shim/shim.cpp") // the only C++ source file
        .include("duckdb_capi/") // vendored duckdb.hpp and header tree
        .flag_if_supported("-std=c++17") // safe on GCC/clang; skipped on MSVC
        .warnings(false) // suppress DuckDB's own internal warnings
        .compile("semantic_views_shim"); // produces libsemantic_views_shim.a, auto-linked

    // Symbol visibility: restrict the cdylib's exported symbols to the DuckDB entry
    // points only. Without this, Rust stdlib symbols leak into the extension binary.
    // Windows: __declspec(dllexport) on the #[no_mangle] entry point handles visibility.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    match target_os.as_str() {
        "linux" => {
            // ELF version script: only DuckDB entry points are globally visible.
            let map_path = format!("{out_dir}/semantic_views.map");
            std::fs::write(
                &map_path,
                "{\n  global:\n    semantic_views_init_c_api;\n    semantic_views_version;\n  local: *;\n};\n",
            )
            .expect("failed to write ELF version script");
            println!("cargo:rustc-link-arg=-Wl,--version-script={map_path}");
        }
        "macos" => {
            // Exported symbols list: only DuckDB entry points are externally visible.
            // macOS uses underscore-prefixed names in the exported symbols file.
            let exp_path = format!("{out_dir}/semantic_views.exp");
            std::fs::write(
                &exp_path,
                "_semantic_views_init_c_api\n_semantic_views_version\n",
            )
            .expect("failed to write macOS exported symbols list");
            println!("cargo:rustc-link-arg=-Wl,-exported_symbols_list,{exp_path}");
        }
        _ => {
            // Windows: no extra flags needed — MSVC dllexport handles visibility.
        }
    }
}
