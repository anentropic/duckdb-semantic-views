//! Guards for `scripts/changelog_add_bullet.py`, which the DuckDB Version Monitor
//! runs to record an automated pin bump under `## [Unreleased]`.
//!
//! Nothing else exercises it: it runs only inside that workflow, and its output
//! is rolled into the release notes by `PublishExtension.yml` without a human
//! re-reading the section. A mis-placed bullet therefore ships verbatim. The
//! v1.5.6 bump did exactly that: the new bullet landed after the *first line* of
//! a wrapped bullet, splitting it in two, and was only caught in the v0.13.0
//! release dry run.
//!
//! The tests run the real script in a throwaway directory against a scratch
//! `CHANGELOG.md` (the script resolves the file relative to its cwd).

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A fresh scratch directory, removed first so a crashed previous run cannot
/// leak state into this one.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sv-changelog-scripts-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run `changelog_add_bullet.py <category> <bullet>` against `changelog` and
/// return the resulting file.
fn add_bullet(name: &str, changelog: &str, category: &str, bullet: &str) -> String {
    let dir = scratch(name);
    let path = dir.join("CHANGELOG.md");
    std::fs::write(&path, changelog).expect("write scratch CHANGELOG.md");
    let out = Command::new("python3")
        .arg(repo_root().join("scripts/changelog_add_bullet.py"))
        .args([category, bullet])
        .current_dir(&dir)
        .output()
        .expect("run python3 (is it on PATH?)");
    assert!(
        out.status.success(),
        "changelog_add_bullet.py failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result = std::fs::read_to_string(&path).expect("read scratch CHANGELOG.md");
    let _ = std::fs::remove_dir_all(&dir);
    result
}

const TAIL: &str = "## [0.1.0] - 2026-01-01\n\n### Added\n\n- Old entry.\n";

#[test]
fn bullet_is_appended_after_a_wrapped_bullet_not_inside_it() {
    let before = format!(
        "# Changelog\n\n## [Unreleased]\n\n### Changed\n\n\
         - **First change** spans\n  several lines\n  of text.\n\n{TAIL}"
    );
    let after = add_bullet("wrapped", &before, "Changed", "Pin bumped.");
    assert!(
        after.contains("- **First change** spans\n  several lines\n  of text.\n- Pin bumped.\n"),
        "new bullet must follow the whole wrapped bullet, got:\n{after}"
    );
}

#[test]
fn bullet_is_appended_after_the_last_of_several_wrapped_bullets() {
    let before = format!(
        "# Changelog\n\n## [Unreleased]\n\n### Changed\n\n\
         - One\n  continued.\n- Two\n  continued.\n\n### Fixed\n\n- A fix.\n\n{TAIL}"
    );
    let after = add_bullet("several", &before, "Changed", "Pin bumped.");
    assert!(
        after.contains("- One\n  continued.\n- Two\n  continued.\n- Pin bumped.\n\n### Fixed"),
        "new bullet must follow the last wrapped bullet of its section, got:\n{after}"
    );
}

#[test]
fn rerunning_with_the_same_bullet_is_a_no_op() {
    let before =
        format!("# Changelog\n\n## [Unreleased]\n\n### Changed\n\n- Pin bumped.\n\n{TAIL}");
    let after = add_bullet("idempotent", &before, "Changed", "Pin bumped.");
    assert_eq!(after, before);
}

#[test]
fn placeholder_is_replaced_by_a_new_section() {
    let before =
        format!("# Changelog\n\n## [Unreleased]\n\n_No unreleased changes yet._\n\n{TAIL}");
    let after = add_bullet("placeholder", &before, "Changed", "Pin bumped.");
    assert_eq!(
        after,
        format!("# Changelog\n\n## [Unreleased]\n\n### Changed\n\n- Pin bumped.\n\n{TAIL}")
    );
}

#[test]
fn missing_section_is_created_in_canonical_order() {
    let before = format!(
        "# Changelog\n\n## [Unreleased]\n\n### Added\n\n- New.\n\n### Fixed\n\n- A fix.\n\n{TAIL}"
    );
    let after = add_bullet("order", &before, "Changed", "Pin bumped.");
    assert!(
        after.contains("### Added\n\n- New.\n\n### Changed\n\n- Pin bumped.\n\n### Fixed"),
        "### Changed must sit between Added and Fixed, got:\n{after}"
    );
}
