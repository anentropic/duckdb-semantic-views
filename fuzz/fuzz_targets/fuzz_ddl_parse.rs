#![no_main]
use libfuzzer_sys::fuzz_target;
use semantic_views::parse::{detect_semantic_view_ddl, validate_and_rewrite, PARSE_DETECTED};

fuzz_target!(|data: &[u8]| {
    // Reject invalid UTF-8 — not a crash, just out of scope for this target.
    let Ok(query) = std::str::from_utf8(data) else {
        return;
    };

    // detect_semantic_view_ddl must never panic regardless of input.
    let detected = detect_semantic_view_ddl(query);

    // If detected as our DDL, validate_and_rewrite must also never panic.
    if detected == PARSE_DETECTED {
        match validate_and_rewrite(query) {
            Ok(Some(sql)) => {
                // Rewritten SQL must start with the expected prefix.
                assert!(
                    sql.starts_with("SELECT * FROM "),
                    "Rewritten SQL does not start with 'SELECT * FROM ': {sql}"
                );
            }
            Ok(None) => {
                // Detection/rewrite disagreement — not a panic, document if seen.
            }
            Err(_) => {
                // Parse error — acceptable, not a panic.
            }
        }
    }
});
