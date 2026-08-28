//! Build-plumbing guards for the two decisions that pick *which DuckDB* a
//! build talks to: the engine the C++ shim is compiled against
//! (`scripts/ensure_amalgamation.py`) and the engine the sqllogictest runner is
//! installed from (`DUCKDB_TEST_VERSION` in the `Makefile`).
//!
//! Both are invisible to every other test in this repo — `cargo test` links a
//! bundled DuckDB and never reads `cpp/include/`, and sqllogictest only ever
//! sees whichever engine the harness already chose. The failure they guard
//! against is therefore not a wrong answer but a wrong *build*, and it shows up
//! only in a downstream distribution's CI. The Haybarn build (an alternative
//! DuckDB distribution that builds community extensions against its own engine
//! fork) failed `make configure` on every platform, because this Makefile
//! pinned `DUCKDB_TEST_VERSION` to an upstream release number that exists in no
//! index of theirs — and had it got past that, it would have compiled the shim
//! against upstream headers describing a different engine than the one loading
//! the result.
//!
//! The tests drive the real `Makefile` and the real script in throwaway
//! directories: no network (the release path is served over `file://`), no
//! engine checkout (the generator is a stub that records what it was asked
//! for), and no dependence on this repo's own `cpp/include/` state.

use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Canonical pinned version, e.g. `v1.5.5`. Read rather than hardcoded so a
/// version bump does not turn these guards red.
fn target_version() -> String {
    std::fs::read_to_string(repo_root().join(".duckdb-version"))
        .expect("read .duckdb-version")
        .trim()
        .to_string()
}

/// A fresh scratch directory, removed first so a crashed previous run cannot
/// leak state into this one.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sv-build-config-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// `Makefile`: which DuckDB the test runner is installed from
// ---------------------------------------------------------------------------

/// Copies the real `Makefile` into a scratch project with *stub* ci-tools
/// makefiles, so `make` can expand its variables without the submodule and
/// without any of base.Makefile's own definitions colouring the result.
fn make_project(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::copy(repo_root().join("Makefile"), dir.join("Makefile")).expect("copy Makefile");
    std::fs::copy(
        repo_root().join(".duckdb-version"),
        dir.join(".duckdb-version"),
    )
    .expect("copy .duckdb-version");
    for stub in ["base.Makefile", "rust.Makefile"] {
        write(
            &dir.join("extension-ci-tools/makefiles/c_api_extensions")
                .join(stub),
            "# stub for tests/build_config.rs\n",
        );
    }
    // `include`d rather than appended so the Makefile under test stays byte-identical.
    write(
        &dir.join("print.mk"),
        "include Makefile\nprint-%:\n\t@echo '$($*)'\n",
    );
    dir
}

/// Expands `var` in the scratch project with `env` applied on top.
fn make_var(dir: &Path, var: &str, env: &[(&str, &str)]) -> String {
    let mut cmd = Command::new("make");
    cmd.arg("-f").arg("print.mk").arg(format!("print-{var}"));
    cmd.current_dir(dir);
    // The ambient environment must not leak into the expansion: CI and local
    // shells may already export the very variables under test.
    for key in ["DUCKDB_GIT_VERSION", "DUCKDB_TEST_VERSION"] {
        cmd.env_remove(key);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    let out = cmd.output().expect("run make (is `make` on PATH?)");
    assert!(
        out.status.success(),
        "make failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Locally and in upstream CI the runner must be pinned to the release this
/// extension targets, so the engine under test cannot drift from the engine the
/// binary was built for.
#[test]
fn test_runner_pin_applies_to_the_upstream_engine() {
    let dir = make_project("pin-upstream");
    let numeric = target_version().trim_start_matches('v').to_string();

    // No harness: a local `make configure`.
    assert_eq!(make_var(&dir, "DUCKDB_TEST_VERSION", &[]), numeric);
    // Upstream community-extensions CI, which names the release verbatim.
    assert_eq!(
        make_var(
            &dir,
            "DUCKDB_TEST_VERSION",
            &[("DUCKDB_GIT_VERSION", &target_version())]
        ),
        numeric
    );
}

/// A harness building against some *other* engine (a distribution fork, a
/// nightly `main`) ships its own test runner under its own versioning scheme.
/// Pinning our release number there resolves to a version that does not exist
/// and fails `make configure` — the Haybarn breakage this guards.
#[test]
fn test_runner_pin_skipped_for_a_foreign_engine() {
    let dir = make_project("pin-foreign");
    for engine in ["haybarn-v1.5.5-rc1", "main"] {
        assert_eq!(
            make_var(
                &dir,
                "DUCKDB_TEST_VERSION",
                &[("DUCKDB_GIT_VERSION", engine)]
            ),
            "",
            "DUCKDB_TEST_VERSION must be left to the harness when building against {engine}"
        );
    }
}

/// Even on the upstream engine an explicit value from the environment wins, so
/// a harness can always name the runner build it wants.
#[test]
fn test_runner_pin_yields_to_an_explicit_override() {
    let dir = make_project("pin-override");
    assert_eq!(
        make_var(
            &dir,
            "DUCKDB_TEST_VERSION",
            &[("DUCKDB_TEST_VERSION", "1.5.5rc1")]
        ),
        "1.5.5rc1"
    );
}

// ---------------------------------------------------------------------------
// `scripts/ensure_amalgamation.py`: which DuckDB the C++ shim is compiled against
// ---------------------------------------------------------------------------

const ENGINE_MARKER: &str = "FROM-ENGINE-SOURCE-TREE";
const RELEASE_MARKER: &str = "FROM-RELEASE-ZIP";

/// A scratch repo root: `.duckdb-version` plus an empty `cpp/include/`.
fn script_project(name: &str) -> PathBuf {
    let dir = scratch(name);
    write(
        &dir.join(".duckdb-version"),
        &format!("{}\n", target_version()),
    );
    std::fs::create_dir_all(dir.join("cpp/include")).expect("create cpp/include");
    dir
}

fn amalgamation_text(version: &str, marker: &str) -> (String, String) {
    (
        format!(
            "#pragma once\n#define DUCKDB_AMALGAMATION 1\n#define DUCKDB_SOURCE_ID \"stub\"\n\
             #define DUCKDB_VERSION \"{version}\"\n#define DUCKDB_MAJOR_VERSION 1\n// {marker}\n"
        ),
        format!("// duckdb.cpp {marker}\n"),
    )
}

/// Plants a stub engine source tree: just enough for the script to recognise it
/// (`scripts/amalgamation.py`) and for us to see what it was asked to produce.
/// `emit_version` is what the stub generator labels its output with — normally
/// whatever `OVERRIDE_GIT_DESCRIBE` says, which is how the real generator
/// behaves.
fn plant_engine_tree(root: &Path, emit_version: Option<&str>) {
    let label = match emit_version {
        Some(fixed) => format!("{fixed:?}"),
        None => "os.environ.get('OVERRIDE_GIT_DESCRIBE', 'v0.0.0').split('-')[0]".to_string(),
    };
    let (hpp, cpp) = amalgamation_text("{version}", ENGINE_MARKER);
    write(
        &root.join("duckdb/scripts/amalgamation.py"),
        &format!(
            "import os, pathlib\n\
             version = {label}\n\
             out = pathlib.Path('src/amalgamation')\n\
             out.mkdir(parents=True, exist_ok=True)\n\
             (out / 'duckdb.hpp').write_text(f{hpp:?})\n\
             (out / 'duckdb.cpp').write_text(f{cpp:?})\n"
        ),
    );
}

/// Builds a release zip on disk and returns the `file://` URL for it, so the
/// download path is exercised without a network.
fn plant_release_zip(root: &Path) -> String {
    let zip = root.join("release/libduckdb-src.zip");
    std::fs::create_dir_all(zip.parent().expect("parent")).expect("create release dir");
    let (hpp, cpp) = amalgamation_text(&target_version(), RELEASE_MARKER);
    const MAKE_ZIP: &str = "import zipfile, sys\n\
         with zipfile.ZipFile(sys.argv[1], 'w') as z:\n    \
             z.writestr('duckdb.hpp', sys.argv[2])\n    \
             z.writestr('duckdb.cpp', sys.argv[3])\n";
    let status = Command::new("python3")
        .arg("-c")
        .arg(MAKE_ZIP)
        .arg(&zip)
        .arg(hpp)
        .arg(cpp)
        .status()
        .expect("run python3 (is it on PATH?)");
    assert!(status.success(), "failed to build the stub release zip");
    format!("file://{}", zip.display())
}

fn run_script(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new("python3")
        .arg(repo_root().join("scripts/ensure_amalgamation.py"))
        .arg("--repo-root")
        .arg(root)
        .args(args)
        .output()
        .expect("run scripts/ensure_amalgamation.py")
}

fn run_script_ok(root: &Path, args: &[&str]) -> String {
    let out = run_script(root, args);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "ensure_amalgamation.py failed\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// The default path for a developer and for upstream CI: no engine tree, so the
/// pinned release is downloaded, installed and stamped with the version.
#[test]
fn release_is_installed_when_no_engine_tree_is_present() {
    let root = script_project("release");
    let url = plant_release_zip(&root);

    run_script_ok(&root, &["--url", &url]);

    assert!(read(&root.join("cpp/include/duckdb.hpp")).contains(RELEASE_MARKER));
    assert!(read(&root.join("cpp/include/duckdb.cpp")).contains(RELEASE_MARKER));
    assert_eq!(
        read(&root.join("cpp/include/.amalgamation_id")).trim(),
        target_version()
    );
}

/// The fix for the fork build: when the harness has supplied the engine the
/// extension will be loaded into, the amalgamation is generated from *that*
/// tree rather than downloaded from upstream.
#[test]
fn engine_source_tree_wins_over_the_release_download() {
    let root = script_project("engine-tree");
    plant_engine_tree(&root, None);
    // A URL that cannot possibly resolve: reaching the download path fails the test.
    let out = run_script_ok(&root, &["--url", "file:///nonexistent/libduckdb-src.zip"]);

    assert!(read(&root.join("cpp/include/duckdb.hpp")).contains(ENGINE_MARKER));
    assert!(read(&root.join("cpp/include/duckdb.cpp")).contains(ENGINE_MARKER));
    assert!(
        out.contains("generating the amalgamation from"),
        "expected generation to be reported, got: {out}"
    );
}

/// The generator is told which release to label its output with, so a fork tag
/// (`haybarn-v1.5.5-rc1`) that `git describe --match 'v*.*.*'` cannot see still
/// yields headers labelled with the DuckDB release the fork is based on.
#[test]
fn generation_labels_the_output_with_the_targeted_release() {
    let root = script_project("engine-label");
    plant_engine_tree(&root, None);
    run_script_ok(&root, &["--url", "file:///nonexistent.zip"]);

    let hpp = read(&root.join("cpp/include/duckdb.hpp"));
    assert!(
        hpp.contains(&format!("#define DUCKDB_VERSION \"{}\"", target_version())),
        "generated header carries the wrong version: {}",
        hpp.lines().take(6).collect::<Vec<_>>().join(" | ")
    );
}

/// An engine tree that is *not* the release we target would produce a binary
/// whose shim disagrees with the engine loading it. That must stop the build
/// loudly rather than compile.
#[test]
fn a_mismatched_engine_tree_is_fatal() {
    let root = script_project("engine-mismatch");
    plant_engine_tree(&root, Some("v1.4.0"));

    let out = run_script(&root, &["--url", "file:///nonexistent.zip"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "a mismatched engine tree must fail");
    assert!(
        stderr.contains("v1.4.0") && stderr.contains(&target_version()),
        "the error must name both versions, got: {stderr}"
    );
    assert!(
        !root.join("cpp/include/duckdb.hpp").exists(),
        "nothing may be installed from a mismatched engine tree"
    );
}

/// Switching engines re-installs; switching back is a cache hit. Without the
/// provenance stamp the second call is a silent no-op — both amalgamations
/// label themselves with the same `DUCKDB_VERSION`, which is exactly how a fork
/// build would end up linking upstream headers.
#[test]
fn switching_engines_reinstalls_and_switching_back_restores() {
    let root = script_project("engine-switch");
    let url = plant_release_zip(&root);

    run_script_ok(&root, &["--url", &url]);
    assert!(read(&root.join("cpp/include/duckdb.hpp")).contains(RELEASE_MARKER));

    plant_engine_tree(&root, None);
    run_script_ok(&root, &["--url", &url]);
    assert!(
        read(&root.join("cpp/include/duckdb.hpp")).contains(ENGINE_MARKER),
        "an engine tree appearing later must replace the release install"
    );

    // Back to the release: served from the cache the first run populated.
    std::fs::remove_dir_all(root.join("duckdb")).expect("remove engine tree");
    let out = run_script_ok(&root, &["--url", "file:///nonexistent.zip"]);
    assert!(read(&root.join("cpp/include/duckdb.hpp")).contains(RELEASE_MARKER));
    assert!(
        out.contains("restoring from cache"),
        "expected the cached release, got: {out}"
    );
}

/// A repeat build must not re-download, re-generate, or re-copy: `build.rs`
/// reruns on `cpp/include/duckdb.cpp`'s mtime, so a needless copy costs a ~10
/// min rebuild of the amalgamation translation unit.
#[test]
fn a_matching_stamp_is_a_no_op() {
    let root = script_project("stamp-noop");
    let url = plant_release_zip(&root);
    run_script_ok(&root, &["--url", &url]);

    let cpp = root.join("cpp/include/duckdb.cpp");
    let before = std::fs::metadata(&cpp)
        .expect("stat")
        .modified()
        .expect("mtime");

    // Any attempt to fetch or generate would fail against this URL.
    run_script_ok(&root, &["--url", "file:///nonexistent.zip"]);

    let after = std::fs::metadata(&cpp)
        .expect("stat")
        .modified()
        .expect("mtime");
    assert_eq!(
        before, after,
        "the installed amalgamation must not be touched"
    );
}

/// Checkouts that predate the stamp already have the pinned release in place.
/// They adopt it instead of re-installing, so the stamp's arrival does not cost
/// every existing checkout a full C++ rebuild.
#[test]
fn an_unstamped_release_install_is_adopted_in_place() {
    let root = script_project("stamp-adopt");
    let (hpp, cpp) = amalgamation_text(&target_version(), RELEASE_MARKER);
    write(&root.join("cpp/include/duckdb.hpp"), &hpp);
    write(&root.join("cpp/include/duckdb.cpp"), &cpp);
    let installed = root.join("cpp/include/duckdb.cpp");
    let before = std::fs::metadata(&installed)
        .expect("stat")
        .modified()
        .expect("mtime");

    run_script_ok(&root, &["--url", "file:///nonexistent.zip"]);

    assert_eq!(
        read(&root.join("cpp/include/.amalgamation_id")).trim(),
        target_version()
    );
    let after = std::fs::metadata(&installed)
        .expect("stat")
        .modified()
        .expect("mtime");
    assert_eq!(before, after, "an adopted install must not be rewritten");
}
