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
//
// Windows Win32 macro patching: duckdb.cpp includes <windows.h> mid-file, which defines
// macros that conflict with DuckDB C++ identifiers (GetObject -> GetObjectA, interface ->
// struct). WIN32_LEAN_AND_MEAN/NOGDI do not reliably suppress these across all Windows SDK
// versions. On Windows, build.rs generates a patched copy of duckdb.cpp in OUT_DIR with
// explicit #undef blocks inserted after each <windows.h> include.

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
        println!("cargo:rerun-if-changed=cpp/include/duckdb.cpp");

        let is_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");

        // On Windows, generate a patched copy of duckdb.cpp in OUT_DIR.
        // duckdb.cpp is gitignored and re-downloaded from DuckDB releases in CI,
        // so patches must be applied at build time, not committed.
        //
        // The patch inserts explicit #undef blocks for Win32 macros that conflict
        // with DuckDB C++ identifiers, following the same pattern DuckDB already
        // uses for CreateDirectory/MoveFile/RemoveDirectory in the same file.
        let duckdb_cpp_path = if is_windows {
            patch_duckdb_cpp_for_windows()
        } else {
            "cpp/include/duckdb.cpp".to_string()
        };

        let mut build = cc::Build::new();
        build
            .cpp(true)
            .std("c++17")
            .include("cpp/include")
            .file(&duckdb_cpp_path)
            .file("cpp/src/shim.cpp")
            .warnings(false); // Suppress warnings from DuckDB amalgamation

        if is_windows {
            // MSVC preprocessor defines applied before any source is compiled.
            // WIN32_LEAN_AND_MEAN — reduces Windows headers included by windows.h
            // NOMINMAX — prevents min/max macros that break std::numeric_limits<T>::max()
            // NOGDI — defense-in-depth: asks wingdi.h to skip GetObject macro definition
            // DUCKDB_STATIC_BUILD — prevents DUCKDB_API expanding to __declspec(dllimport)
            //
            // Note: GetObject and `interface` are also explicitly #undef-d in the patched
            // duckdb.cpp (see patch_duckdb_cpp_for_windows below), because NOGDI and
            // WIN32_LEAN_AND_MEAN are not reliable across all Windows SDK configurations.
            build.define("WIN32_LEAN_AND_MEAN", None);
            build.define("NOMINMAX", None);
            build.define("NOGDI", None);
            build.define("DUCKDB_STATIC_BUILD", None);
            // /bigobj: duckdb.cpp (~300K lines) exceeds MSVC's default 65,536-section COFF limit.
            // flag_if_supported is a no-op on non-MSVC toolchains.
            build.flag_if_supported("/bigobj");
        }

        build.compile("semantic_views_shim");
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
        "windows" => {
            // rstrtmgr: DuckDB v1.4.4 added duckdb::AdditionalLockInfo which calls
            // the Windows Restart Manager API (RmStartSession, RmEndSession,
            // RmRegisterResources, RmGetList). These are defined in rstrtmgr.lib,
            // which is not linked by default. Without this, the link fails with
            // LNK2019/LNK1120 unresolved external symbol errors.
            // rstrtmgr.lib ships with every Windows SDK so no installation is required.
            // Symbol visibility (__declspec(dllexport)) is handled by the #[no_mangle]
            // entry point — no extra linker flags needed for that.
            println!("cargo:rustc-link-lib=rstrtmgr");
        }
        _ => {
            // Other platforms need no extra link flags.
        }
    }
}

/// Generate a patched copy of duckdb.cpp in OUT_DIR for Windows builds.
///
/// duckdb.cpp includes <windows.h> mid-file (after all DuckDB declarations are
/// already processed from duckdb.hpp). The windows.h include can define macros that
/// conflict with identifiers in the DuckDB C++ implementation code that follows:
///
/// - `GetObject` → `GetObjectA` (wingdi.h): conflicts with ComplexJSON::GetObject
///   (line ~36327) and ObjectCache::GetObject (line ~37656).
/// - `interface` → `struct` (objbase.h): conflicts with MultiFileReader `interface`
///   variable names (line ~65873+).
///
/// The fix adds explicit `#undef` blocks after each <windows.h> include, following
/// the same pattern DuckDB already uses for CreateDirectory/MoveFile/RemoveDirectory
/// in the same file. We patch at build time (not at commit time) because duckdb.cpp
/// is gitignored and re-downloaded from DuckDB releases in CI.
///
/// Returns the path to the patched file (or the original if already patched).
#[cfg(feature = "extension")]
fn patch_duckdb_cpp_for_windows() -> String {
    let orig = "cpp/include/duckdb.cpp";
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let patched = format!("{out_dir}/duckdb_patched.cpp");

    let content = std::fs::read_to_string(orig).expect(
        "failed to read cpp/include/duckdb.cpp — \
             run 'make ensure_amalgamation' or 'just update-headers' to download it",
    );

    // Idempotent: if already patched (e.g. local dev copy with manual edits), use as-is.
    // We check for one of our undef markers to detect prior patching.
    if content.contains("undef GetObject") {
        std::fs::write(&patched, &content).expect("failed to write duckdb_patched.cpp to OUT_DIR");
        return patched;
    }

    // --- Patch 1: after the first <windows.h> include (DLL loading helpers block) ---
    //
    // DuckDB <= 1.4.x had two separate <windows.h> includes. The first was in a
    // DLL loading helpers block, marked by "// Platform-specific helpers". DuckDB 1.5.0
    // consolidated to a single <windows.h> include (handled by Patch 2 below), so
    // this patch is expected to be a no-op for DuckDB >= 1.5.0.
    //
    // Original context (DuckDB 1.4.x duckdb.cpp ~line 25363-25372):
    //   #endif // defined(_WIN32)
    //
    //   // Platform-specific helpers        <-- patch inserted before this
    let win32_undef_block = "\
        // Undefine Windows macros that conflict with DuckDB C++ identifiers.\n\
        // GetObject (wingdi.h) -> clashes with ComplexJSON::GetObject, ObjectCache::GetObject\n\
        // interface (objbase.h) -> clashes with MultiFileReader 'interface' variable names\n\
        #if defined(_WIN32)\n\
        #  ifdef GetObject\n\
        #    undef GetObject\n\
        #  endif\n\
        #  ifdef interface\n\
        #    undef interface\n\
        #  endif\n\
        #endif\n\n";

    let patch1_before = "#endif // defined(_WIN32)\n\n// Platform-specific helpers";
    let patch1_after =
        format!("#endif // defined(_WIN32)\n\n{win32_undef_block}// Platform-specific helpers");

    let content = if content.contains(patch1_before) {
        content.replace(patch1_before, &patch1_after)
    } else {
        // Expected for DuckDB >= 1.5.0 where the first windows.h include was removed.
        content
    };

    // --- Patch 2: after the second <windows.h> include (file system helpers block) ---
    //
    // Original context (duckdb.cpp ~line 38078-38094):
    //   #if defined(_WIN32)
    //   #ifndef NOMINMAX ... #endif
    //   #ifndef _WINSOCKAPI_ ... #endif
    //   #include <windows.h>
    //   #undef CreateDirectory    <-- DuckDB already undefs some macros here
    //   #undef MoveFile
    //   #undef RemoveDirectory
    //                             <-- we append our undefs after RemoveDirectory
    //   #endif
    //   #else
    //   #include <sys/mman.h>
    //
    // A second windows.h include can re-define interface (used as C++ identifier name
    // starting at ~line 65873). GetObject undef is defensive here.
    let patch2_before = "#undef CreateDirectory\n#undef MoveFile\n#undef RemoveDirectory\n\n#endif\n\n#else\n#include <sys/mman.h>";
    let patch2_after = "#undef CreateDirectory\n#undef MoveFile\n#undef RemoveDirectory\n\
        #ifdef GetObject\n\
        #  undef GetObject\n\
        #endif\n\
        #ifdef interface\n\
        #  undef interface\n\
        #endif\n\n\
        #endif\n\n#else\n#include <sys/mman.h>";

    let content = if content.contains(patch2_before) {
        content.replace(patch2_before, patch2_after)
    } else {
        println!(
            "cargo:warning=duckdb.cpp Win32 patch 2 skipped: expected marker not found. \
                  This may indicate a DuckDB version change — verify interface macro is not \
                  causing build failures."
        );
        content
    };

    std::fs::write(&patched, content).expect("failed to write duckdb_patched.cpp to OUT_DIR");

    patched
}
