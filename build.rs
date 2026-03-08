// build.rs
// Cargo build script — compiles C++ shim and restricts exported symbols when building the
// loadable extension.
//
// Design: Only the `extension` feature triggers C++ compilation and symbol visibility
// configuration. During `cargo test` (default/bundled feature), this script exits immediately.
//
// C++ compilation: The cc crate compiles shim.cpp against the vendored DuckDB amalgamation
// header (cpp/include/duckdb.hpp). This produces a static library that gets linked into the
// cdylib extension binary.
//
// Symbol visibility: restricts the exported symbols of the cdylib to the C_STRUCT entry
// point (semantic_views_init_c_api) on Linux and macOS. Without this, Rust stdlib symbols
// leak into the extension binary.
// Note: semantic_views_version is appended by the CI post-build script after compilation;
// it is NOT compiled into the binary and must not appear in the symbol list.

fn main() {
    // Only configure C++ compilation and symbol visibility when building the loadable
    // extension binary. CARGO_FEATURE_EXTENSION is set by Cargo when `--features extension`
    // is passed. During `cargo test` (uses default/bundled feature), this block is skipped.
    if std::env::var("CARGO_FEATURE_EXTENSION").is_err() {
        return;
    }

    // Compile the DuckDB amalgamation source + C++ shim.
    // duckdb.cpp provides all DuckDB C++ symbol definitions (constructors, RTTI,
    // vtables) so the shim can use ParserExtension, TableFunction, etc. without
    // relying on symbol export from the host process (Python DuckDB compiles with
    // -fvisibility=hidden). Symbol visibility in the cdylib is restricted below
    // so these definitions stay internal to the extension binary.
    //
    // First build: ~2-5 min (duckdb.cpp is ~300K lines). Cached by cc crate after.
    //
    // The cc crate is an optional build-dependency gated on the `extension` feature.
    #[cfg(feature = "extension")]
    {
        // Ensure cargo re-runs this script when the C++ shim changes.
        // The cc crate should emit rerun-if-changed automatically, but adding
        // explicit directives ensures changes to shim.cpp always trigger rebuilds.
        println!("cargo:rerun-if-changed=cpp/src/shim.cpp");
        println!("cargo:rerun-if-changed=cpp/include/duckdb.hpp");

        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .include("cpp/include")
            .file("cpp/include/duckdb.cpp")
            .file("cpp/src/shim.cpp")
            .warnings(false) // Suppress warnings from DuckDB amalgamation
            .compile("semantic_views_shim");
    }

    // Symbol visibility: restrict the cdylib's exported symbols to the DuckDB CPP
    // entry point only. Without this, Rust stdlib symbols leak into the extension binary.
    //
    // Exports: semantic_views_init_c_api (Rust entry point, C_STRUCT ABI)
    //
    // Windows: __declspec(dllexport) on the #[no_mangle] entry point handles visibility.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    match target_os.as_str() {
        "linux" => {
            // Linux symbol visibility: rustc already generates a --version-script for
            // cdylib targets. Adding a second version script causes GNU ld (especially
            // gcc-toolset-14 on arm64) to reject the link with "anonymous version tag
            // cannot be combined with other version tags". Instead, we use
            // --dynamic-list which cooperates with rustc's version script and restricts
            // visible symbols to our entry point.
            let dynlist_path = format!("{out_dir}/semantic_views.dynlist");
            std::fs::write(&dynlist_path, "{\n  semantic_views_init_c_api;\n};\n")
                .expect("failed to write dynamic list");
            println!("cargo:rustc-link-arg=-Wl,--dynamic-list={dynlist_path}");
        }
        "macos" => {
            // Exported symbols list: only the entry point is externally visible.
            // macOS uses underscore-prefixed names in the exported symbols file.
            // Note: semantic_views_version is appended by the CI post-build script
            // (extension-ci-tools); it does NOT exist in the compiled binary and must
            // not be listed here — the linker would fail with "undefined symbol".
            let exp_path = format!("{out_dir}/semantic_views.exp");
            std::fs::write(&exp_path, "_semantic_views_init_c_api\n")
                .expect("failed to write macOS exported symbols list");
            println!("cargo:rustc-link-arg=-Wl,-exported_symbols_list,{exp_path}");
        }
        _ => {
            // Windows: no extra flags needed — MSVC dllexport handles visibility.
        }
    }
}
