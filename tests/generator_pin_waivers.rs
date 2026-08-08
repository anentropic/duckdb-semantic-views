//! Meta-test: every inert generator default in a proptest harness must carry a
//! written justification.
//!
//! **Why this file exists.** The single most reliable predictor of a silent
//! wrong-number bug in this project has been a query-shaping field pinned at its
//! inert default in every generator. `where_clause: None` in all five harnesses
//! is how EXP-9 and EXP-10 reached `main`; `facts: vec![]` in all of them is how
//! EXP-28 did. CLAUDE.md has warned about the pattern in prose since PBT-6 —
//! and PBT-13 landed anyway, in a *brand new* harness written two commits after
//! that warning, by authors who had read it.
//!
//! Prose did not hold. The lesson (recorded in
//! `_notes/proactive-defect-discovery.md` §2.5) is that a rule survives when a
//! machine checks it, so this test makes a pin a *decision with a record*
//! rather than a default nobody had to think about.
//!
//! **What it does NOT claim.** A `// PIN:` comment is not evidence the pin is
//! correct — it is evidence somebody chose it and said why. That is the whole
//! ambition here: the failure mode being prevented is the pin nobody noticed,
//! not the pin someone weighed and kept.
//!
//! **To satisfy it:** either vary the field in the generator (better), or put
//! `// PIN: <reason>` on the same line or the line above, ideally naming the
//! TECH-DEBT entry that tracks the gap (#66 is the coverage-axis ledger).

use std::fs;
use std::path::Path;

/// Field spellings that are inert: a generator "covering" them at this value
/// exercises no behaviour the feature was built for.
///
/// Deliberately NOT listed: `output_type: None`, which is now the only value
/// the field can take (TECH-DEBT #68 withdrew it from YAML import), so a pin
/// there is forced rather than chosen — flagging it would be pure noise, and a
/// meta-test that cries wolf gets suppressed rather than fixed.
const INERT_PINS: &[&str] = &["where_clause: None", "facts: vec![]"];

/// A pin is waived by `// PIN:` on the same line or the line immediately above.
const WAIVER: &str = "// PIN:";

#[test]
fn every_inert_generator_pin_carries_a_written_waiver() {
    let tests_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut offenders: Vec<String> = Vec::new();
    let mut waived = 0usize;
    let mut scanned_files = 0usize;

    let mut entries: Vec<_> = fs::read_dir(&tests_dir)
        .expect("read tests/ directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "rs")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with("_proptest.rs"))
        })
        .collect();
    entries.sort();

    for path in &entries {
        scanned_files += 1;
        let text = fs::read_to_string(path).expect("read harness source");
        let lines: Vec<&str> = text.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let code = line.trim_start();
            // A pin mentioned inside a comment is documentation, not a pin.
            if code.starts_with("//") {
                continue;
            }
            let Some(pin) = INERT_PINS.iter().find(|p| line.contains(**p)) else {
                continue;
            };

            let waived_here =
                line.contains(WAIVER) || (i > 0 && lines[i - 1].trim_start().starts_with(WAIVER));

            if waived_here {
                waived += 1;
            } else {
                offenders.push(format!(
                    "{}:{}: `{}` pinned with no `{WAIVER} <reason>` waiver",
                    path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                    i + 1,
                    pin
                ));
            }
        }
    }

    // Anti-vacuity: if the scan finds no harnesses, or the pin spellings drift
    // so nothing matches, this test would pass while checking nothing — the
    // exact failure mode it exists to prevent. Both floors are facts about the
    // repository today, so a change that breaks them is a change that must
    // revisit this test.
    assert!(
        scanned_files >= 6,
        "expected at least 6 *_proptest.rs harnesses, scanned {scanned_files} — \
         has the naming convention changed? This test is checking nothing."
    );
    assert!(
        waived + offenders.len() >= 10,
        "expected at least 10 inert-pin sites across the harnesses, found {} — \
         the INERT_PINS spellings have probably drifted from the code, so this \
         test is checking nothing.",
        waived + offenders.len()
    );

    assert!(
        offenders.is_empty(),
        "{} inert generator pin(s) without a written waiver.\n\n\
         A field pinned at its inert default is how EXP-9, EXP-10 and EXP-28 \
         reached main: the harness looks like it covers the feature, and covers \
         nothing. Either vary it in the generator, or record why not with \
         `{WAIVER} <reason>` on the same line or the line above (name the \
         TECH-DEBT entry if one tracks the gap — #66 is the coverage ledger).\n\n\
         {}",
        offenders.len(),
        offenders.join("\n")
    );
}
